#![allow(dead_code)]

extern crate std;

use crate::*;
#[cfg(feature = "async")]
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};
use embedded_hal::{
    delay::DelayNs,
    i2c::{ErrorKind, ErrorType, I2c, NoAcknowledgeSource, Operation, SevenBitAddress},
};
#[cfg(feature = "async")]
use std::pin::pin;
use std::vec::Vec;

// ---- bus mock (same approach as ull-sht3x) ----

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct MockI2cError(ErrorKind);

impl MockI2cError {
    const fn other() -> Self {
        Self(ErrorKind::Other)
    }

    const fn no_ack() -> Self {
        Self(ErrorKind::NoAcknowledge(NoAcknowledgeSource::Address))
    }
}

impl embedded_hal::i2c::Error for MockI2cError {
    fn kind(&self) -> ErrorKind {
        self.0
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum Event {
    Write { address: u8, bytes: Vec<u8> },
    Read { address: u8, len: usize },
}

#[derive(Debug)]
enum ReadResponse {
    Data(Vec<u8>),
    Error(MockI2cError),
}

#[derive(Debug, Default)]
struct MockI2c {
    events: Vec<Event>,
    reads: Vec<ReadResponse>,
    write_error: Option<MockI2cError>,
}

#[allow(dead_code)]
impl MockI2c {
    fn new(reads: impl IntoIterator<Item = Vec<u8>>) -> Self {
        Self {
            events: Vec::new(),
            reads: reads.into_iter().map(ReadResponse::Data).collect(),
            write_error: None,
        }
    }

    fn with_read_responses(reads: impl IntoIterator<Item = ReadResponse>) -> Self {
        Self {
            events: Vec::new(),
            reads: reads.into_iter().collect(),
            write_error: None,
        }
    }

    fn failing_writes() -> Self {
        Self {
            write_error: Some(MockI2cError::other()),
            ..Self::default()
        }
    }

    fn nacking_writes() -> Self {
        Self {
            write_error: Some(MockI2cError(ErrorKind::NoAcknowledge(
                NoAcknowledgeSource::Data,
            ))),
            ..Self::default()
        }
    }

    /// Only the write payloads, in order.
    fn writes(&self) -> Vec<Vec<u8>> {
        self.events
            .iter()
            .filter_map(|event| match event {
                Event::Write { bytes, .. } => Some(bytes.clone()),
                Event::Read { .. } => None,
            })
            .collect()
    }

    fn run(&mut self, address: u8, op: MockOp<'_>) -> core::result::Result<(), MockI2cError> {
        match op {
            MockOp::Read(buffer) => {
                self.events.push(Event::Read {
                    address,
                    len: buffer.len(),
                });
                match self.reads.remove(0) {
                    ReadResponse::Data(read) => buffer.copy_from_slice(&read),
                    ReadResponse::Error(error) => return Err(error),
                }
            }
            MockOp::Write(bytes) => {
                self.events.push(Event::Write {
                    address,
                    bytes: bytes.to_vec(),
                });
                if let Some(error) = self.write_error {
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}

enum MockOp<'a> {
    Read(&'a mut [u8]),
    Write(&'a [u8]),
}

impl ErrorType for MockI2c {
    type Error = MockI2cError;
}

impl I2c<SevenBitAddress> for MockI2c {
    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> core::result::Result<(), Self::Error> {
        for operation in operations {
            match operation {
                Operation::Read(buffer) => self.run(address, MockOp::Read(buffer))?,
                Operation::Write(bytes) => self.run(address, MockOp::Write(bytes))?,
            }
        }
        Ok(())
    }
}

#[cfg(feature = "async")]
impl embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress> for MockI2c {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [embedded_hal_async::i2c::Operation<'_>],
    ) -> core::result::Result<(), Self::Error> {
        for operation in operations {
            match operation {
                embedded_hal_async::i2c::Operation::Read(buffer) => {
                    self.run(address, MockOp::Read(buffer))?
                }
                embedded_hal_async::i2c::Operation::Write(bytes) => {
                    self.run(address, MockOp::Write(bytes))?
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
struct MockDelay {
    delayed_ms: Vec<u32>,
}

impl DelayNs for MockDelay {
    fn delay_ns(&mut self, _ns: u32) {}

    fn delay_ms(&mut self, ms: u32) {
        self.delayed_ms.push(ms);
    }
}

#[cfg(feature = "async")]
#[derive(Debug, Default)]
struct MockAsyncDelay {
    delayed_ms: Vec<u32>,
}

#[cfg(feature = "async")]
impl embedded_hal_async::delay::DelayNs for MockAsyncDelay {
    async fn delay_ns(&mut self, _ns: u32) {}

    async fn delay_ms(&mut self, ms: u32) {
        self.delayed_ms.push(ms);
    }
}

#[cfg(feature = "async")]
fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let mut future = pin!(future);

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => core::hint::spin_loop(),
        }
    }
}
// ---- end mock ----

fn write_event(bytes: &[u8]) -> Event {
    Event::Write {
        address: 0x46,
        bytes: Vec::from(bytes),
    }
}

fn read_event(len: usize) -> Event {
    Event::Read { address: 0x46, len }
}

/// Registers read: one request write followed by a read of `len` bytes.
fn read_events(register: u8, len: usize) -> [Event; 2] {
    [write_event(&[register]), read_event(len)]
}

#[test]
fn addresses() {
    assert_eq!(Address::default().as_u8(), 0x46);
    assert_eq!(Address::ALTERNATE.as_u8(), 0x47);
    assert_eq!(Address::custom(0x47), Some(Address::ALTERNATE));
    assert_eq!(Address::custom(0x76), None);
}

#[test]
fn default_config_matches_register_reset_values_where_expected() {
    let config = Config::default();
    // 0x70 is the ODR_CONFIG reset value: odr = 0x1C (1 Hz), standby, deep standby allowed.
    assert_eq!(config.odr_register(PowerMode::Standby), 0x70);
    // press_en (bit 6) | osr_p = x4 (010 << 3) | osr_t = x1.
    assert_eq!(config.osr_register(), 0x50);
    assert_eq!(config.iir_register(), 0x00);
}

#[test]
fn config_encoding() {
    let config = Config {
        osr_temperature: Oversampling::X8,
        osr_pressure: Oversampling::X32,
        pressure_enabled: true,
        odr: OutputDataRate::Hz10,
        iir: IirFilter::Coeff7,
        deep_standby_disabled: true,
    };
    // 0x40 | (5 << 3) | 3
    assert_eq!(config.osr_register(), 0x6B);
    // deep_dis 0x80 | (0x17 << 2) | normal
    assert_eq!(config.odr_register(PowerMode::Normal), 0xDD);
    assert_eq!(config.odr_register(PowerMode::Forced), 0xDE);
    assert_eq!(config.odr_register(PowerMode::Continuous), 0xDF);
    // set_iir_p bits 2:0, set_iir_t bits 5:3.
    assert_eq!(config.iir_register(), 0x1B);

    let temperature_only = Config {
        pressure_enabled: false,
        ..Config::default()
    };
    assert_eq!(temperature_only.osr_register(), 0x10);
}

#[test]
fn odr_table_matches_datasheet() {
    assert_eq!(OutputDataRate::Hz240.as_u8(), 0x00);
    assert_eq!(OutputDataRate::Hz1.as_u8(), 0x1C);
    assert_eq!(OutputDataRate::Hz0_125.as_u8(), 0x1F);
    assert_eq!(OutputDataRate::Hz218_537.millihertz(), 218_537);
    assert_eq!(OutputDataRate::Hz50_056.millihertz(), 50_056);
    assert_eq!(OutputDataRate::Hz0_125.millihertz(), 125);
    for value in 0..=0x1F {
        assert_eq!(OutputDataRate::from_u8(value).unwrap().as_u8(), value);
    }
    assert_eq!(OutputDataRate::from_u8(0x20), None);
}

#[test]
fn forced_measurement_delay_follows_conversion_times() {
    use crate::types::forced_measurement_delay_ms;
    // 4 ms start-up + (1.0 + 2.9) ms * 1.05 = 8.095 ms -> 9 ms.
    assert_eq!(forced_measurement_delay_ms(0x50), 9);
    // Temperature only, x1: 4 + 1.05 = 5.05 ms -> 6 ms.
    assert_eq!(forced_measurement_delay_ms(0x00), 6);
    // x128 / x128: 4 + (80.4 + 20.8) * 1.05 = 110.26 ms -> 111 ms.
    assert_eq!(forced_measurement_delay_ms(0x7F | 0x40), 111);
    // Pressure bits are ignored when press_en is clear: 4 + 20.8 * 1.05 = 25.84.
    assert_eq!(forced_measurement_delay_ms(0x3F), 26);
}

#[test]
fn interrupt_reset_config_is_0x35() {
    let config = InterruptConfig::default();
    assert_eq!(config, InterruptConfig::RESET);
    assert_eq!(config.output, InterruptOutput::OpenDrain);
    assert_eq!(config.polarity, InterruptPolarity::ActiveLow);
    assert_eq!(config.mode, InterruptMode::Latched);
    // Datasheet p. 52: INT_CONFIG reset value 0x35 (INT pin disabled).
    assert_eq!(
        config.to_register(false),
        Some(crate::types::INT_CONFIG_RESET)
    );
    assert_eq!(config.to_register(false), Some(0x35));
    assert_eq!(config.to_register(true), Some(0x3D));
    assert_eq!(InterruptConfig::from_register(0x35), (config, false));
    assert_eq!(InterruptConfig::from_register(0x3D), (config, true));
}

#[test]
fn interrupt_config_encoding() {
    let pushpull = InterruptConfig {
        mode: InterruptMode::Pulsed,
        polarity: InterruptPolarity::ActiveHigh,
        output: InterruptOutput::PushPull,
        drive_strength: 3,
    };
    // drive 3 | int_en | int_pol, no int_od, no int_mode.
    assert_eq!(pushpull.to_register(true), Some(0x3A));
    assert_eq!(InterruptConfig::from_register(0x3A), (pushpull, true));

    // The drive MSB is only allowed with open drain.
    let strong_open_drain = InterruptConfig {
        drive_strength: 0x0F,
        ..InterruptConfig::RESET
    };
    assert_eq!(strong_open_drain.to_register(false), Some(0xF5));
    assert_eq!(
        InterruptConfig {
            drive_strength: 8,
            ..pushpull
        }
        .to_register(true),
        None
    );
    assert_eq!(
        InterruptConfig {
            drive_strength: 16,
            ..InterruptConfig::RESET
        }
        .to_register(true),
        None
    );
}

#[test]
fn interrupt_status_decoding() {
    let status = InterruptStatus(0x11);
    assert!(status.data_ready() && status.por());
    assert!(!status.fifo_full() && !status.fifo_threshold() && !status.pressure_out_of_range());
    assert!(InterruptStatus(0x02).fifo_full());
    assert!(InterruptStatus(0x04).fifo_threshold());
    assert!(InterruptStatus(0x08).pressure_out_of_range());
}

#[test]
fn converts_raw_data_at_25_degc_and_standard_pressure() {
    // 25 degC = 25 * 65536 = 0x190000; 101325 Pa = 101325 * 64 = 0x62F340.
    let raw = RawMeasurement::from_bytes([0x00, 0x00, 0x19, 0x40, 0xF3, 0x62]);
    assert_eq!(raw.temperature, 25 * 65_536);
    assert_eq!(raw.pressure, 101_325 * 64);
    assert_eq!(raw.temperature_millicelsius(), 25_000);
    assert_eq!(raw.pressure_pa(), 101_325);
    assert_eq!(raw.pressure_millipascal(), 101_325_000);
    assert_eq!(raw.temperature_celsius(), 25.0);
    assert_eq!(raw.pressure_pa_f32(), 101_325.0);
    assert_eq!(
        raw.to_fixed_point(),
        FixedPointMeasurement {
            temperature_millicelsius: 25_000,
            pressure_millipascal: 101_325_000,
        }
    );
    assert_eq!(
        raw.to_measurement(),
        Measurement {
            temperature_celsius: 25.0,
            pressure_pa: 101_325.0
        }
    );
}

#[test]
fn pressure_keeps_sub_pascal_resolution() {
    // One LSb is 1/64 Pa = 15.625 mPa; 32 LSb = 0.5 Pa.
    let raw = RawMeasurement {
        temperature: 0,
        pressure: 101_325 * 64 + 32,
    };
    assert_eq!(raw.pressure_pa(), 101_325);
    assert_eq!(raw.pressure_millipascal(), 101_325_500);
    assert_eq!(raw.pressure_pa_f32(), 101_325.5);
    let one_lsb = RawMeasurement {
        temperature: 1,
        pressure: 1,
    };
    assert_eq!(one_lsb.pressure_millipascal(), 15); // 15.625 floored
}

#[test]
fn temperature_is_sign_extended() {
    // 0xFF0000 as signed 24 bit is -65536 = -1 degC.
    let raw = RawMeasurement::from_bytes([0x00, 0x00, 0xFF, 0, 0, 0]);
    assert_eq!(raw.temperature, -65_536);
    assert_eq!(raw.temperature_millicelsius(), -1_000);
    assert_eq!(raw.temperature_celsius(), -1.0);
    // 0x800000 is the most negative value, -128 degC.
    let raw = RawMeasurement::from_bytes([0x00, 0x00, 0x80, 0, 0, 0]);
    assert_eq!(raw.temperature_millicelsius(), -128_000);
    // Reset value of the data registers, 0x7F7F7F / 65536 = 127.498 degC.
    let raw = RawMeasurement::from_bytes([0x7F; 6]);
    assert_eq!(raw.temperature_millicelsius(), 127_498);
    // Half a degree below zero is floored: -0.5 degC = -32768 LSb.
    let raw = RawMeasurement::from_bytes([0x00, 0x80, 0xFF, 0, 0, 0]);
    assert_eq!(raw.temperature_millicelsius(), -500);
}

#[test]
fn chip_id_reads_register_0x01() {
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x50])]));
    assert_eq!(sensor.chip_id().unwrap(), 0x50);
    assert_eq!(sensor.release().events, Vec::from(read_events(0x01, 1)));
}

#[test]
fn check_chip_id_rejects_other_ids() {
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x50]), Vec::from([0x51])]));
    assert_eq!(sensor.check_chip_id(), Ok(()));
    assert_eq!(sensor.check_chip_id(), Err(Error::UnexpectedChipId(0x51)));
}

#[test]
fn soft_reset_writes_cmd_and_waits() {
    let mut sensor = Bmp580::new(MockI2c::default());
    let mut delay = MockDelay::default();
    sensor.soft_reset(&mut delay).unwrap();
    assert_eq!(
        sensor.release().writes(),
        Vec::from([Vec::from([0x7E, 0xB6])])
    );
    assert_eq!(delay.delayed_ms, Vec::from([2]));
}

#[test]
fn soft_reset_tolerates_the_missing_ack() {
    let mut sensor = Bmp580::new(MockI2c::nacking_writes());
    let mut delay = MockDelay::default();
    assert_eq!(sensor.soft_reset(&mut delay), Ok(()));
    assert_eq!(delay.delayed_ms, Vec::from([2]));

    let mut sensor = Bmp580::new(MockI2c::failing_writes());
    assert_eq!(
        sensor.soft_reset(&mut MockDelay::default()),
        Err(Error::Bus(MockI2cError::other()))
    );
}

#[test]
fn init_runs_the_post_power_up_checks() {
    let mut sensor = Bmp580::new(MockI2c::new([
        Vec::from([0x50]),
        Vec::from([0x02]),
        Vec::from([0x10]),
    ]));
    let mut delay = MockDelay::default();
    sensor.init(&mut delay).unwrap();
    let mut expected = Vec::from([write_event(&[0x7E, 0xB6])]);
    expected.extend(read_events(0x01, 1));
    expected.extend(read_events(0x28, 1));
    expected.extend(read_events(0x27, 1));
    assert_eq!(sensor.release().events, expected);
}

#[test]
fn init_reports_nvm_problems_and_wrong_chip() {
    for status in [0x00, 0x04, 0x06] {
        let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x50]), Vec::from([status])]));
        assert_eq!(
            sensor.init(&mut MockDelay::default()),
            Err(Error::Nvm),
            "{status:#x}"
        );
    }
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x00])]));
    assert_eq!(
        sensor.init(&mut MockDelay::default()),
        Err(Error::UnexpectedChipId(0))
    );
}

#[test]
fn power_mode_reads_the_actual_mode() {
    for (register, mode) in [
        (0x70, PowerMode::Standby),
        (0x71, PowerMode::Normal),
        (0x72, PowerMode::Forced),
        (0xF3, PowerMode::Continuous),
    ] {
        let mut sensor = Bmp580::new(MockI2c::new([Vec::from([register])]));
        assert_eq!(sensor.power_mode().unwrap(), mode);
    }
}

#[test]
fn set_power_mode_writes() {
    for (before, mode, after) in [
        (0x70, PowerMode::Normal, 0x71),
        (0xF0, PowerMode::Forced, 0xF2),
        (0x70, PowerMode::Continuous, 0x73),
        (0x73, PowerMode::Standby, 0x70),
    ] {
        let mut sensor = Bmp580::new(MockI2c::new([Vec::from([before])]));
        sensor.set_power_mode(mode).unwrap();
        assert_eq!(
            sensor.release().writes(),
            Vec::from([Vec::from([0x37]), Vec::from([0x37, after])])
        );
    }
}

#[test]
fn set_standby_waits_for_transition() {
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x71])]));
    let mut delay = MockDelay::default();
    sensor.set_standby(&mut delay).unwrap();
    assert_eq!(delay.delayed_ms, Vec::from([3]));
    assert_eq!(sensor.release().writes()[1], Vec::from([0x37, 0x70]));
}

#[test]
fn configure_from_standby_writes_iir_osr_odr_in_order() {
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x70])]));
    let mut delay = MockDelay::default();
    sensor.configure(&mut delay, &Config::default()).unwrap();
    assert_eq!(
        sensor.release().writes(),
        Vec::from([
            Vec::from([0x37]),
            Vec::from([0x31, 0x00]),
            Vec::from([0x36, 0x50]),
            Vec::from([0x37, 0x70]),
        ])
    );
    assert!(delay.delayed_ms.is_empty());
}

#[test]
fn configure_leaves_other_modes_via_standby_first() {
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x71]), Vec::from([0x71])]));
    let mut delay = MockDelay::default();
    let config = Config {
        odr: OutputDataRate::Hz10,
        iir: IirFilter::Coeff3,
        ..Config::default()
    };
    sensor.configure(&mut delay, &config).unwrap();
    assert_eq!(
        sensor.release().writes(),
        Vec::from([
            Vec::from([0x37]),
            Vec::from([0x37]),
            Vec::from([0x37, 0x70]),
            Vec::from([0x31, 0x12]),
            Vec::from([0x36, 0x50]),
            Vec::from([0x37, 0x5C]),
        ])
    );
    assert_eq!(delay.delayed_ms, Vec::from([3]));
}

#[test]
fn measure_forced_from_standby() {
    let data = Vec::from([0x00, 0x00, 0x19, 0x40, 0xF3, 0x62]);
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x50, 0x70]), data]));
    let mut delay = MockDelay::default();
    let raw = sensor.measure_forced(&mut delay).unwrap();
    assert_eq!(raw.to_fixed_point().temperature_millicelsius, 25_000);
    assert_eq!(delay.delayed_ms, Vec::from([9]));
    let mut expected = Vec::from(read_events(0x36, 2));
    expected.push(write_event(&[0x37, 0x72]));
    expected.extend(read_events(0x1D, 6));
    assert_eq!(sensor.release().events, expected);
}

#[test]
fn measure_forced_goes_through_standby_and_keeps_odr_bits() {
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x7F, 0xF1]), Vec::from([0; 6])]));
    let mut delay = MockDelay::default();
    sensor.measure_forced(&mut delay).unwrap();
    assert_eq!(delay.delayed_ms, Vec::from([3, 111]));
    assert_eq!(
        sensor.release().writes(),
        Vec::from([
            Vec::from([0x36]),
            Vec::from([0x37, 0xF0]),
            Vec::from([0x37, 0xF2]),
            Vec::from([0x1D]),
        ])
    );
}

#[test]
fn read_measurement_is_one_six_byte_burst() {
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([1, 2, 3, 4, 5, 6])]));
    let raw = sensor.read_measurement().unwrap();
    assert_eq!(raw.temperature, 0x03_02_01);
    assert_eq!(raw.pressure, 0x06_05_04);
    assert_eq!(sensor.release().events, Vec::from(read_events(0x1D, 6)));
}

#[test]
fn read_temperature_only_is_a_three_byte_burst() {
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x00, 0x00, 0xFF])]));
    assert_eq!(sensor.read_temperature_raw().unwrap(), -65_536);
    assert_eq!(sensor.release().events, Vec::from(read_events(0x1D, 3)));
}

#[test]
fn odr_osr_valid_reads_osr_eff_bit7() {
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x80]), Vec::from([0x12])]));
    assert!(sensor.odr_osr_valid().unwrap());
    assert!(!sensor.odr_osr_valid().unwrap());
    assert_eq!(sensor.release().writes()[0], Vec::from([0x38]));
}

#[test]
fn data_ready_interrupt_with_reset_pin_settings_keeps_open_drain_active_low() {
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x01])]));
    sensor
        .configure_data_ready_interrupt(&InterruptConfig::default())
        .unwrap();
    assert_eq!(
        sensor.release().writes(),
        Vec::from([
            Vec::from([0x15, 0x00]),
            Vec::from([0x27]),
            // 0x3D = reset value 0x35 with int_en set: still open drain, active low.
            Vec::from([0x14, 0x3D]),
            Vec::from([0x15, 0x01]),
        ])
    );
}

#[test]
fn data_ready_interrupt_can_be_made_push_pull_only_explicitly() {
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x00])]));
    sensor
        .configure_data_ready_interrupt(&InterruptConfig {
            output: InterruptOutput::PushPull,
            mode: InterruptMode::Pulsed,
            polarity: InterruptPolarity::ActiveHigh,
            ..InterruptConfig::default()
        })
        .unwrap();
    assert_eq!(sensor.release().writes()[2], Vec::from([0x14, 0x3A]));
}

#[test]
fn invalid_interrupt_config_writes_nothing() {
    let bad = InterruptConfig {
        output: InterruptOutput::PushPull,
        drive_strength: 9,
        ..InterruptConfig::default()
    };
    let mut sensor = Bmp580::new(MockI2c::default());
    assert_eq!(
        sensor.configure_data_ready_interrupt(&bad),
        Err(Error::InvalidConfig)
    );
    assert_eq!(
        sensor.set_interrupt_config(&bad, true),
        Err(Error::InvalidConfig)
    );
    assert!(sensor.release().events.is_empty());
}

#[test]
fn interrupt_config_and_status_accessors() {
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x35]), Vec::from([0x11])]));
    assert_eq!(
        sensor.interrupt_config().unwrap(),
        (InterruptConfig::RESET, false)
    );
    let status = sensor.interrupt_status().unwrap();
    assert!(status.data_ready() && status.por());
    sensor
        .set_interrupt_config(&InterruptConfig::RESET, false)
        .unwrap();
    sensor.disable_interrupts().unwrap();
    let writes = sensor.release().writes();
    assert_eq!(writes[2], Vec::from([0x14, 0x35]));
    assert_eq!(writes[3], Vec::from([0x15, 0x00]));
}

#[test]
fn raw_register_access() {
    let mut sensor = Bmp580::with_address(
        MockI2c::new([Vec::from([0xAB]), Vec::from([1, 2, 3])]),
        Address::ALTERNATE,
    );
    assert_eq!(sensor.read_register(0x30).unwrap(), 0xAB);
    let mut buffer = [0u8; 3];
    sensor.read_registers(0x32, &mut buffer).unwrap();
    assert_eq!(buffer, [1, 2, 3]);
    sensor.write_register(0x34, 0x32).unwrap();
    let i2c = sensor.release();
    assert!(matches!(i2c.events[0], Event::Write { address: 0x47, .. }));
    assert_eq!(
        i2c.writes(),
        Vec::from([
            Vec::from([0x30]),
            Vec::from([0x32]),
            Vec::from([0x34, 0x32])
        ])
    );
}

#[test]
fn bus_errors_propagate() {
    let mut sensor = Bmp580::new(MockI2c::with_read_responses([ReadResponse::Error(
        MockI2cError::no_ack(),
    )]));
    assert_eq!(sensor.chip_id(), Err(Error::Bus(MockI2cError::no_ack())));
    let mut sensor = Bmp580::new(MockI2c::failing_writes());
    assert_eq!(
        sensor.write_register(0x37, 0),
        Err(Error::Bus(MockI2cError::other()))
    );
}

#[test]
fn formats_errors_for_logs() {
    assert_eq!(
        std::format!("{}", Error::<MockI2cError>::UnexpectedChipId(0x51)),
        "unexpected chip id 0x51, expected 0x50"
    );
}

fn assert_send<T: Send>() {}

#[test]
fn driver_is_send_when_i2c_is_send() {
    assert_send::<Bmp580<MockI2c>>();
}

#[cfg(feature = "async")]
#[test]
fn blocking_and_async_apis_produce_identical_bus_traffic() {
    fn reads() -> Vec<Vec<u8>> {
        Vec::from([
            Vec::from([0x50]),       // chip id (init)
            Vec::from([0x02]),       // status
            Vec::from([0x10]),       // int status
            Vec::from([0x71]),       // configure: power mode
            Vec::from([0x71]),       // configure: set_standby RMW
            Vec::from([0x50, 0x70]), // forced: osr, odr
            Vec::from([0x00, 0x00, 0x19, 0x40, 0xF3, 0x62]),
            Vec::from([0x7F, 0xF1]), // forced from non-standby
            Vec::from([0x01, 0x02, 0x03, 0x04, 0x05, 0x06]),
            Vec::from([0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F]), // read_measurement
            Vec::from([0x70]),                               // set_power_mode
            Vec::from([0x01]),                               // interrupt setup: INT_STATUS
            Vec::from([0x11]),                               // interrupt status
            Vec::from([0x00, 0x00, 0xFF]),                   // temperature
            Vec::from([0x80]),                               // odr valid
            Vec::from([0x35]),                               // interrupt config
            Vec::from([0x50]),                               // chip id
        ])
    }

    let config = Config {
        odr: OutputDataRate::Hz10,
        iir: IirFilter::Coeff3,
        ..Config::default()
    };

    let mut delay = MockDelay::default();
    let mut sensor = Bmp580::new(MockI2c::new(reads()));
    sensor.init(&mut delay).unwrap();
    sensor.configure(&mut delay, &config).unwrap();
    let a = sensor.measure_forced(&mut delay).unwrap();
    let b = sensor.measure_forced(&mut delay).unwrap();
    let c = sensor.read_measurement().unwrap();
    sensor.set_power_mode(PowerMode::Normal).unwrap();
    sensor
        .configure_data_ready_interrupt(&InterruptConfig::default())
        .unwrap();
    let status = sensor.interrupt_status().unwrap();
    let t = sensor.read_temperature_raw().unwrap();
    let valid = sensor.odr_osr_valid().unwrap();
    let (int_config, enabled) = sensor.interrupt_config().unwrap();
    sensor.disable_interrupts().unwrap();
    sensor.write_register(0x34, 0x32).unwrap();
    sensor.soft_reset(&mut delay).unwrap();
    sensor.check_chip_id().unwrap();
    let blocking_i2c = sensor.release();

    let mut async_delay = MockAsyncDelay::default();
    let mut sensor = Bmp580::new(MockI2c::new(reads()));
    block_on(sensor.init_async(&mut async_delay)).unwrap();
    block_on(sensor.configure_async(&mut async_delay, &config)).unwrap();
    assert_eq!(
        block_on(sensor.measure_forced_async(&mut async_delay)).unwrap(),
        a
    );
    assert_eq!(
        block_on(sensor.measure_forced_async(&mut async_delay)).unwrap(),
        b
    );
    assert_eq!(block_on(sensor.read_measurement_async()).unwrap(), c);
    block_on(sensor.set_power_mode_async(PowerMode::Normal)).unwrap();
    block_on(sensor.configure_data_ready_interrupt_async(&InterruptConfig::default())).unwrap();
    assert_eq!(block_on(sensor.interrupt_status_async()).unwrap(), status);
    assert_eq!(block_on(sensor.read_temperature_raw_async()).unwrap(), t);
    assert_eq!(block_on(sensor.odr_osr_valid_async()).unwrap(), valid);
    assert_eq!(
        block_on(sensor.interrupt_config_async()).unwrap(),
        (int_config, enabled)
    );
    block_on(sensor.disable_interrupts_async()).unwrap();
    block_on(sensor.write_register_async(0x34, 0x32)).unwrap();
    block_on(sensor.soft_reset_async(&mut async_delay)).unwrap();
    block_on(sensor.check_chip_id_async()).unwrap();
    let async_i2c = sensor.release();

    assert_eq!(blocking_i2c.events, async_i2c.events);
    assert_eq!(delay.delayed_ms, async_delay.delayed_ms);
    assert!(async_i2c.reads.is_empty());
}

#[cfg(feature = "async")]
#[test]
fn async_soft_reset_tolerates_the_missing_ack() {
    let mut sensor = Bmp580::new(MockI2c::nacking_writes());
    let mut delay = MockAsyncDelay::default();
    assert_eq!(block_on(sensor.soft_reset_async(&mut delay)), Ok(()));
    assert_eq!(delay.delayed_ms, Vec::from([2]));
}
