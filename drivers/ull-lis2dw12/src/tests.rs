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

    /// Register writes in order (payloads of two or more bytes; the one-byte
    /// register-pointer writes of register reads are left out).
    fn writes(&self) -> Vec<Vec<u8>> {
        self.events
            .iter()
            .filter_map(|event| match event {
                Event::Write { bytes, .. } if bytes.len() > 1 => Some(bytes.clone()),
                Event::Write { .. } | Event::Read { .. } => None,
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
        address: 0x19,
        bytes: Vec::from(bytes),
    }
}

fn read_event(len: usize) -> Event {
    Event::Read { address: 0x19, len }
}

/// Registers read: one request write followed by a read of `len` bytes.
fn read_events(register: u8, len: usize) -> [Event; 2] {
    [write_event(&[register]), read_event(len)]
}

fn sensor(reads: impl IntoIterator<Item = u8>) -> Lis2dw12<MockI2c> {
    Lis2dw12::new(MockI2c::new(
        reads.into_iter().map(|byte| Vec::from([byte])),
    ))
}

fn events(i2c: &MockI2c) -> Vec<Event> {
    i2c.events.clone()
}

#[test]
fn addresses() {
    assert_eq!(Address::default().as_u8(), 0x19);
    assert_eq!(Address::ALTERNATE.as_u8(), 0x18);
    assert_eq!(Address::custom(0x18), Some(Address::ALTERNATE));
    assert_eq!(Address::custom(0x19), Some(Address::DEFAULT));
    assert_eq!(Address::custom(0x1A), None);
    let sensor = Lis2dw12::with_address(MockI2c::default(), Address::ALTERNATE);
    assert_eq!(sensor.address(), 0x18);
    assert_eq!(Lis2dw12::from(MockI2c::default()).address(), 0x19);
}

#[test]
fn config_encodes_ctrl1_and_ctrl6() {
    // ODR 0101 (100 Hz), MODE 01 (HP), LP_MODE 00 -> 0101_0100.
    // BW 01, FS 10 (+-8 g), LOW_NOISE -> 01_10_0_1_00 = 0x64.
    let config = Config {
        odr: OutputDataRate::Hz100,
        mode: OperatingMode::HighPerformance,
        low_power_mode: LowPowerMode::Mode1,
        full_scale: FullScale::G8,
        bandwidth: BandwidthFilter::OdrDiv4,
        low_noise: true,
    };
    assert_eq!(config.ctrl1(), 0x54);
    assert_eq!(config.ctrl6(), 0x64);
    assert_eq!(Config::from_registers(0x54, 0x64), Some(config));

    // ODR 0001, MODE 10 (single conversion), LP_MODE 11; BW 11, FS 11.
    let config = Config {
        odr: OutputDataRate::Hz1_6,
        mode: OperatingMode::SingleConversion,
        low_power_mode: LowPowerMode::Mode4,
        full_scale: FullScale::G16,
        bandwidth: BandwidthFilter::OdrDiv20,
        low_noise: false,
    };
    assert_eq!(config.ctrl1(), 0b0001_1011);
    assert_eq!(config.ctrl6(), 0b1111_0000);
    assert_eq!(Config::from_registers(0x1B, 0xF0), Some(config));
}

#[test]
fn default_config_matches_reset_registers() {
    let config = Config::default();
    assert_eq!(config.ctrl1(), 0x00);
    assert_eq!(config.ctrl6(), 0x00);
    assert_eq!(Config::from_registers(0, 0), Some(config));
}

#[test]
fn reserved_register_values_do_not_decode() {
    assert_eq!(Config::from_registers(0b0001_1100, 0), None); // MODE 11
    assert_eq!(Config::from_registers(0b1010_0000, 0), None); // ODR 1010
    assert_eq!(
        OutputDataRate::from_bits(0b1001),
        Some(OutputDataRate::Hz1600)
    );
    assert_eq!(OutputDataRate::from_bits(0b1111), None);
}

#[test]
fn resolution_follows_mode_and_low_power_mode() {
    let mut config = Config::default();
    assert_eq!(config.resolution(), Resolution::Bits12);
    config.low_power_mode = LowPowerMode::Mode2;
    assert_eq!(config.resolution(), Resolution::Bits14);
    config.low_power_mode = LowPowerMode::Mode1;
    config.mode = OperatingMode::SingleConversion;
    assert_eq!(config.resolution(), Resolution::Bits12);
    config.mode = OperatingMode::HighPerformance;
    assert_eq!(config.resolution(), Resolution::Bits14);
}

#[test]
fn output_data_rate_depends_on_mode() {
    use OperatingMode::{HighPerformance, LowPower};
    assert_eq!(OutputDataRate::PowerDown.millihertz(LowPower), 0);
    assert_eq!(OutputDataRate::Hz1_6.millihertz(LowPower), 1_600);
    assert_eq!(OutputDataRate::Hz1_6.millihertz(HighPerformance), 12_500);
    assert_eq!(OutputDataRate::Hz12_5.millihertz(LowPower), 12_500);
    assert_eq!(OutputDataRate::Hz100.millihertz(HighPerformance), 100_000);
    assert_eq!(OutputDataRate::Hz400.millihertz(LowPower), 200_000);
    assert_eq!(OutputDataRate::Hz400.millihertz(HighPerformance), 400_000);
    assert_eq!(OutputDataRate::Hz800.millihertz(HighPerformance), 800_000);
    assert_eq!(OutputDataRate::Hz1600.millihertz(LowPower), 200_000);
    assert_eq!(
        OutputDataRate::Hz1600.millihertz(HighPerformance),
        1_600_000
    );
    let config = Config {
        odr: OutputDataRate::Hz50,
        ..Config::default()
    };
    assert_eq!(config.rate_millihertz(), 50_000);
}

#[test]
fn acceleration_converts_to_micro_g() {
    // 0x4000 is 16384; 14-bit digit = 4096; 4096 * 244 ug = 999_424 ug.
    let raw = RawAcceleration::from_bytes([0x00, 0x40, 0x00, 0xC0, 0x04, 0x00]);
    assert_eq!(
        raw,
        RawAcceleration {
            x: 16384,
            y: -16384,
            z: 4
        }
    );
    assert_eq!(
        raw.to_micro_g(FullScale::G2, Resolution::Bits14),
        [999_424, -999_424, 244]
    );
    // +-4 g: 488 ug/digit.
    assert_eq!(
        raw.to_micro_g(FullScale::G4, Resolution::Bits14),
        [1_998_848, -1_998_848, 488]
    );
    // 12-bit: 0x4000 >> 4 = 1024 digits; +-2 g is 976 ug/digit.
    assert_eq!(
        raw.to_micro_g(FullScale::G2, Resolution::Bits12),
        [999_424, -999_424, 0]
    );
    // +-16 g, 12-bit: 0x1000 >> 4 = 256 digits * 7808 ug.
    let raw = RawAcceleration {
        x: 0x1000,
        y: 0,
        z: -16,
    };
    assert_eq!(
        raw.to_micro_g(FullScale::G16, Resolution::Bits12),
        [1_998_848, 0, -7_808]
    );
    // +-8 g, 14-bit: 976 ug/digit.
    assert_eq!(FullScale::G8.micro_g_per_digit(Resolution::Bits14), 976);
    assert_eq!(FullScale::G16.micro_g_per_digit(Resolution::Bits14), 1_952);
    assert_eq!(FullScale::G4.micro_g_per_digit(Resolution::Bits12), 1_952);
    assert_eq!(FullScale::G8.micro_g_per_digit(Resolution::Bits12), 3_904);
}

#[test]
fn acceleration_converts_to_milli_g() {
    let raw = RawAcceleration {
        x: 0x4000,
        y: -0x4000,
        z: 4,
    };
    let mg = raw.to_milli_g(FullScale::G2, Resolution::Bits14);
    assert!((mg[0] - 999.424).abs() < 0.001);
    assert!((mg[1] + 999.424).abs() < 0.001);
    assert!((mg[2] - 0.244).abs() < 0.0001);
    // Most negative word, 14-bit +-2 g: -8192 digits * 244 ug.
    let raw = RawAcceleration {
        x: i16::MIN,
        y: 0,
        z: 0,
    };
    assert_eq!(
        raw.to_micro_g(FullScale::G2, Resolution::Bits14)[0],
        -1_998_848
    );
}

#[test]
fn single_conversion_times() {
    assert_eq!(LowPowerMode::Mode1.single_conversion_time_us(), 1_200);
    assert_eq!(LowPowerMode::Mode2.single_conversion_time_us(), 1_700);
    assert_eq!(LowPowerMode::Mode3.single_conversion_time_us(), 2_300);
    assert_eq!(LowPowerMode::Mode4.single_conversion_time_us(), 3_550);
}

#[test]
fn who_am_i_reads_register_0f() {
    let mut sensor = sensor([0x44, 0x44]);
    assert_eq!(sensor.who_am_i().unwrap(), 0x44);
    sensor.check_who_am_i().unwrap();
    let mut expected = Vec::from(read_events(0x0F, 1));
    expected.extend(read_events(0x0F, 1));
    assert_eq!(events(&sensor.release()), expected);
}

#[test]
fn wrong_who_am_i_is_reported() {
    let mut sensor = sensor([0x33]);
    assert_eq!(sensor.check_who_am_i(), Err(Error::UnexpectedChipId(0x33)));
}

#[test]
fn soft_reset_writes_ctrl2_and_polls_until_clear() {
    let mut sensor = sensor([0x44, 0x44, 0x04]);
    let mut delay = MockDelay::default();
    sensor.soft_reset(&mut delay).unwrap();
    let i2c = sensor.release();
    let mut expected = Vec::from([write_event(&[0x21, 0x44])]);
    for _ in 0..3 {
        expected.extend(read_events(0x21, 1));
    }
    assert_eq!(events(&i2c), expected);
    assert_eq!(delay.delayed_ms, Vec::from([1, 1, 1]));
}

#[test]
fn soft_reset_times_out_after_ten_polls() {
    let mut sensor = sensor([0x44; 10]);
    let mut delay = MockDelay::default();
    assert_eq!(sensor.soft_reset(&mut delay), Err(Error::ResetTimeout));
    assert_eq!(delay.delayed_ms.len(), 10);
    assert!(sensor.release().reads.is_empty());
}

#[test]
fn init_resets_then_checks_who_am_i() {
    let mut sensor = sensor([0x04, 0x44]);
    let mut delay = MockDelay::default();
    sensor.init(&mut delay).unwrap();
    let mut expected = Vec::from([write_event(&[0x21, 0x44])]);
    expected.extend(read_events(0x21, 1));
    expected.extend(read_events(0x0F, 1));
    assert_eq!(events(&sensor.release()), expected);
    assert_eq!(delay.delayed_ms, Vec::from([1]));
}

#[test]
fn init_fails_on_wrong_chip() {
    let mut sensor = sensor([0x04, 0x00]);
    let mut delay = MockDelay::default();
    assert_eq!(sensor.init(&mut delay), Err(Error::UnexpectedChipId(0x00)));
}

#[test]
fn configure_writes_ctrl6_before_ctrl1() {
    let mut sensor = sensor([]);
    let config = Config {
        odr: OutputDataRate::Hz100,
        mode: OperatingMode::HighPerformance,
        full_scale: FullScale::G8,
        bandwidth: BandwidthFilter::OdrDiv4,
        low_noise: true,
        ..Config::default()
    };
    sensor.configure(&config).unwrap();
    assert_eq!(
        sensor.release().writes(),
        Vec::from([Vec::from([0x25, 0x64]), Vec::from([0x20, 0x54])])
    );
}

#[test]
fn read_config_reads_ctrl1_and_ctrl6() {
    let mut sensor = sensor([0x54, 0x64]);
    let config = sensor.read_config().unwrap();
    assert_eq!(config.odr, OutputDataRate::Hz100);
    assert_eq!(config.full_scale, FullScale::G8);
    let mut expected = Vec::from(read_events(0x20, 1));
    expected.extend(read_events(0x25, 1));
    assert_eq!(events(&sensor.release()), expected);
}

#[test]
fn read_config_rejects_reserved_mode() {
    let mut sensor = sensor([0x1C, 0x00]);
    assert_eq!(sensor.read_config(), Err(Error::InvalidConfig));
}

#[test]
fn power_down_clears_only_the_odr_bits() {
    let mut sensor = sensor([0x57]);
    sensor.power_down().unwrap();
    let i2c = sensor.release();
    assert_eq!(i2c.writes(), Vec::from([Vec::from([0x20, 0x07])]));
}

#[test]
fn block_data_update_is_read_modify_write() {
    let mut sensor = sensor([0x04, 0x0C]);
    sensor.set_block_data_update(true).unwrap();
    sensor.set_block_data_update(false).unwrap();
    assert_eq!(
        sensor.release().writes(),
        Vec::from([Vec::from([0x21, 0x0C]), Vec::from([0x21, 0x04])])
    );
}

#[test]
fn trigger_single_conversion_sets_ctrl3_bits_1_and_0() {
    let mut sensor = sensor([0x08]);
    sensor.trigger_single_conversion().unwrap();
    assert_eq!(
        sensor.release().writes(),
        Vec::from([Vec::from([0x22, 0x0B])])
    );
}

#[test]
fn status_decodes_flags() {
    let mut sensor = sensor([0b0110_0011]);
    let status = sensor.status().unwrap();
    assert!(status.data_ready());
    assert!(status.free_fall());
    assert!(status.sleep_state());
    assert!(status.wake_up());
    assert!(!Status(0).data_ready());
    assert!(!Status(0b1001_1100).wake_up());
    assert_eq!(events(&sensor.release()), Vec::from(read_events(0x27, 1)));
}

#[test]
fn acceleration_is_one_six_byte_burst_from_out_x_l() {
    let mut sensor = Lis2dw12::new(MockI2c::new([Vec::from([
        0x00, 0x40, 0x00, 0xC0, 0x04, 0x00,
    ])]));
    let raw = sensor.read_acceleration().unwrap();
    assert_eq!(
        raw,
        RawAcceleration {
            x: 16384,
            y: -16384,
            z: 4
        }
    );
    assert_eq!(events(&sensor.release()), Vec::from(read_events(0x28, 6)));
}

#[test]
fn acceleration_in_mg_uses_config_scale_and_resolution() {
    let config = Config {
        odr: OutputDataRate::Hz50,
        mode: OperatingMode::HighPerformance,
        full_scale: FullScale::G4,
        ..Config::default()
    };
    let mut sensor = Lis2dw12::new(MockI2c::new([Vec::from([
        0x00, 0x40, 0x00, 0xC0, 0x04, 0x00,
    ])]));
    let mg = sensor.read_acceleration_mg(&config).unwrap();
    // 14-bit +-4 g: 4096 digits * 0.488 mg.
    assert!((mg[0] - 1998.848).abs() < 0.001);
    assert!((mg[1] + 1998.848).abs() < 0.001);
    assert!((mg[2] - 0.488).abs() < 0.0001);
}

#[test]
fn temperature_is_offset_from_25_degrees() {
    let mut sensor = sensor([0xFD, 0x05]);
    assert_eq!(sensor.read_temperature_raw().unwrap(), -3);
    assert_eq!(sensor.read_temperature_celsius().unwrap(), 30);
    let mut expected = Vec::from(read_events(0x26, 1));
    expected.extend(read_events(0x26, 1));
    assert_eq!(events(&sensor.release()), expected);
}

#[test]
fn interrupt_pin_config_encodes_ctrl3_bits() {
    let latched_low = InterruptPinConfig {
        polarity: InterruptPolarity::ActiveLow,
        output: InterruptOutput::PushPull,
        mode: InterruptMode::Latched,
    };
    assert_eq!(latched_low.ctrl3_bits(), 0b0001_1000);
    assert_eq!(InterruptPinConfig::default().ctrl3_bits(), 0);
    let open_drain = InterruptPinConfig {
        polarity: InterruptPolarity::ActiveHigh,
        output: InterruptOutput::OpenDrain,
        mode: InterruptMode::Pulsed,
    };
    assert_eq!(open_drain.ctrl3_bits(), 0b0010_0000);
    assert_eq!(
        InterruptPinConfig::from_ctrl3(0b0011_1000).mode,
        InterruptMode::Latched
    );
    assert_eq!(InterruptPinConfig::from_ctrl3(0b0001_1000), latched_low);
}

#[test]
fn set_interrupt_pin_config_preserves_other_ctrl3_bits() {
    // Self-test ST1 (0x40) and SLP_MODE bits (0x03) must survive.
    let mut sensor = sensor([0x43, 0x7B]);
    let active_low = InterruptPinConfig {
        polarity: InterruptPolarity::ActiveLow,
        output: InterruptOutput::PushPull,
        mode: InterruptMode::Pulsed,
    };
    sensor.set_interrupt_pin_config(&active_low).unwrap();
    sensor
        .set_interrupt_pin_config(&InterruptPinConfig::default())
        .unwrap();
    assert_eq!(
        sensor.release().writes(),
        Vec::from([Vec::from([0x22, 0x4B]), Vec::from([0x22, 0x43])])
    );
}

#[test]
fn interrupt_pin_config_is_read_from_ctrl3() {
    let mut sensor = sensor([0b0000_1000]);
    let config = sensor.interrupt_pin_config().unwrap();
    assert_eq!(config.polarity, InterruptPolarity::ActiveLow);
    assert_eq!(config.output, InterruptOutput::PushPull);
    assert_eq!(config.mode, InterruptMode::Pulsed);
}

#[test]
fn wake_up_threshold_from_milli_g() {
    // +-2 g: 1 LSB = 2000/64 = 31.25 mg, so 500 mg is exactly 16 LSB.
    let config = WakeUpConfig::from_milli_g(FullScale::G2, 500, 2).unwrap();
    assert_eq!(
        config,
        WakeUpConfig {
            threshold: 16,
            duration: 2
        }
    );
    assert_eq!(config.threshold_micro_g(FullScale::G2), 500_000);
    // +-16 g: 1 LSB = 250 mg.
    let config = WakeUpConfig::from_milli_g(FullScale::G16, 500, 0).unwrap();
    assert_eq!(config.threshold, 2);
    // +-4 g: 62.5 mg per LSB; 480 mg rounds to 8 LSB.
    assert_eq!(
        WakeUpConfig::from_milli_g(FullScale::G4, 480, 0)
            .unwrap()
            .threshold,
        8
    );
    // Largest representable value and rounding up to 63.
    assert_eq!(
        WakeUpConfig::from_milli_g(FullScale::G2, 1_969, 0)
            .unwrap()
            .threshold,
        63
    );
}

#[test]
fn wake_up_threshold_out_of_range_is_rejected() {
    assert_eq!(WakeUpConfig::from_milli_g(FullScale::G2, 0, 0), None);
    assert_eq!(WakeUpConfig::from_milli_g(FullScale::G2, 10, 0), None); // rounds to 0
    assert_eq!(WakeUpConfig::from_milli_g(FullScale::G2, 2_000, 0), None); // 64
    assert_eq!(WakeUpConfig::from_milli_g(FullScale::G2, 500, 4), None);
}

#[test]
fn enable_wake_up_interrupt_wire_sequence() {
    // CTRL4 reads 0x01 (DRDY routed), CTRL7 reads 0x00, ALL_INT_SRC 0x00.
    let mut sensor = sensor([0x01, 0x00, 0x00]);
    sensor
        .enable_wake_up_interrupt(&WakeUpConfig {
            threshold: 16,
            duration: 2,
        })
        .unwrap();
    let mut expected = Vec::from([write_event(&[0x34, 0x10]), write_event(&[0x35, 0x40])]);
    expected.extend(read_events(0x23, 1));
    expected.push(write_event(&[0x23, 0x21]));
    expected.extend(read_events(0x3F, 1));
    expected.push(write_event(&[0x3F, 0x20]));
    expected.extend(read_events(0x3B, 1));
    assert_eq!(events(&sensor.release()), expected);
}

#[test]
fn invalid_wake_up_config_touches_nothing() {
    let mut sensor = sensor([]);
    let too_high = WakeUpConfig {
        threshold: 64,
        duration: 0,
    };
    assert_eq!(
        sensor.enable_wake_up_interrupt(&too_high),
        Err(Error::InvalidConfig)
    );
    let too_long = WakeUpConfig {
        threshold: 1,
        duration: 4,
    };
    assert_eq!(
        sensor.enable_wake_up_interrupt(&too_long),
        Err(Error::InvalidConfig)
    );
    assert!(sensor.release().events.is_empty());
}

#[test]
fn disable_wake_up_interrupt_clears_int1_wu_only() {
    let mut sensor = sensor([0x21]);
    sensor.disable_wake_up_interrupt().unwrap();
    assert_eq!(
        sensor.release().writes(),
        Vec::from([Vec::from([0x23, 0x01])])
    );
}

#[test]
fn wake_up_source_decodes_axes() {
    // WU_IA bit 3 and X_WU bit 2.
    let mut sensor = sensor([0b0000_1100, 0b0011_0011]);
    let source = sensor.read_wake_up_source().unwrap();
    assert!(source.wake_up() && source.x());
    assert!(!source.y() && !source.z());
    assert!(!source.sleep_state() && !source.free_fall());
    let source = sensor.read_wake_up_source().unwrap();
    assert!(source.free_fall() && source.sleep_state());
    assert!(source.y() && source.z());
    assert_eq!(sensor.release().events[0], write_event(&[0x38]));
}

#[test]
fn clear_interrupts_reads_all_int_src() {
    let mut sensor = sensor([0b0010_0011]);
    let source = sensor.clear_interrupts().unwrap();
    assert!(source.wake_up() && source.free_fall() && source.sleep_change());
    assert_eq!(events(&sensor.release()), Vec::from(read_events(0x3B, 1)));
    assert!(!AllInterruptSource(0b0001_1100).wake_up());
}

#[test]
fn raw_register_access() {
    let mut sensor = Lis2dw12::new(MockI2c::new([Vec::from([0xAB]), Vec::from([1, 2, 3])]));
    assert_eq!(sensor.read_register(0x30).unwrap(), 0xAB);
    let mut buffer = [0u8; 3];
    sensor.read_registers(0x30, &mut buffer).unwrap();
    assert_eq!(buffer, [1, 2, 3]);
    sensor.write_register(0x33, 0x5A).unwrap();
    let mut expected = Vec::from(read_events(0x30, 1));
    expected.extend(read_events(0x30, 3));
    expected.push(write_event(&[0x33, 0x5A]));
    assert_eq!(events(&sensor.release()), expected);
}

#[test]
fn bus_errors_propagate() {
    let mut sensor = Lis2dw12::new(MockI2c::failing_writes());
    assert_eq!(
        sensor.write_register(0x20, 0),
        Err(Error::Bus(MockI2cError::other()))
    );
    assert_eq!(
        sensor.configure(&Config::default()),
        Err(Error::Bus(MockI2cError::other()))
    );
    let mut sensor = Lis2dw12::new(MockI2c::with_read_responses([ReadResponse::Error(
        MockI2cError::no_ack(),
    )]));
    assert_eq!(sensor.status(), Err(Error::Bus(MockI2cError::no_ack())));
}

#[test]
fn read_failure_during_read_modify_write_writes_nothing() {
    let mut sensor = Lis2dw12::new(MockI2c::with_read_responses([ReadResponse::Error(
        MockI2cError::other(),
    )]));
    assert_eq!(sensor.power_down(), Err(Error::Bus(MockI2cError::other())));
    assert!(sensor.release().writes().is_empty());
}

#[test]
fn formats_errors_for_logs() {
    assert_eq!(
        std::format!("{}", Error::<MockI2cError>::UnexpectedChipId(0x33)),
        "unexpected chip id 0x33, expected 0x44"
    );
    assert_eq!(
        std::format!("{}", Error::<MockI2cError>::ResetTimeout),
        "soft reset did not complete"
    );
}

fn assert_send<T: Send>() {}

#[test]
fn driver_is_send_when_i2c_is_send() {
    assert_send::<Lis2dw12<MockI2c>>();
}

#[cfg(feature = "async")]
#[test]
fn blocking_and_async_apis_produce_identical_bus_traffic() {
    fn reads() -> Vec<Vec<u8>> {
        Vec::from([
            Vec::from([0x44]),                               // soft reset poll (still busy)
            Vec::from([0x04]),                               // soft reset poll (done)
            Vec::from([0x44]),                               // init: who am i
            Vec::from([0x54]),                               // read_config ctrl1
            Vec::from([0x64]),                               // read_config ctrl6
            Vec::from([0x57]),                               // power_down
            Vec::from([0x04]),                               // bdu
            Vec::from([0x00]),                               // single conversion
            Vec::from([0x03]),                               // status
            Vec::from([0x00, 0x40, 0x00, 0xC0, 0x04, 0x00]), // acceleration
            Vec::from([0x00, 0x40, 0x00, 0xC0, 0x04, 0x00]), // acceleration mg
            Vec::from([0xFD]),                               // temperature raw
            Vec::from([0x05]),                               // temperature c
            Vec::from([0x00]),                               // pin config RMW
            Vec::from([0x08]),                               // pin config read
            Vec::from([0x01]),                               // wake-up: CTRL4
            Vec::from([0x00]),                               // wake-up: CTRL7
            Vec::from([0x02]),                               // wake-up: ALL_INT_SRC
            Vec::from([0x21]),                               // disable wake-up
            Vec::from([0x0C]),                               // wake-up source
            Vec::from([0x02]),                               // clear interrupts
            Vec::from([0x44]),                               // who am i
        ])
    }

    let config = Config {
        odr: OutputDataRate::Hz100,
        mode: OperatingMode::HighPerformance,
        ..Config::default()
    };
    let pins = InterruptPinConfig {
        polarity: InterruptPolarity::ActiveLow,
        ..InterruptPinConfig::default()
    };
    let wake = WakeUpConfig {
        threshold: 16,
        duration: 1,
    };

    let mut delay = MockDelay::default();
    let mut sensor = Lis2dw12::new(MockI2c::new(reads()));
    sensor.soft_reset(&mut delay).unwrap();
    sensor.check_who_am_i().unwrap();
    let c = sensor.read_config().unwrap();
    sensor.power_down().unwrap();
    sensor.set_block_data_update(true).unwrap();
    sensor.trigger_single_conversion().unwrap();
    let s = sensor.status().unwrap();
    sensor.configure(&config).unwrap();
    let a = sensor.read_acceleration().unwrap();
    let m = sensor.read_acceleration_mg(&config).unwrap();
    let t = sensor.read_temperature_raw().unwrap();
    let tc = sensor.read_temperature_celsius().unwrap();
    sensor.set_interrupt_pin_config(&pins).unwrap();
    let p = sensor.interrupt_pin_config().unwrap();
    sensor.enable_wake_up_interrupt(&wake).unwrap();
    sensor.disable_wake_up_interrupt().unwrap();
    let w = sensor.read_wake_up_source().unwrap();
    let i = sensor.clear_interrupts().unwrap();
    sensor.write_register(0x33, 0x5A).unwrap();
    let who = sensor.who_am_i().unwrap();
    let blocking_i2c = sensor.release();

    let mut async_delay = MockAsyncDelay::default();
    let mut sensor = Lis2dw12::new(MockI2c::new(reads()));
    block_on(sensor.soft_reset_async(&mut async_delay)).unwrap();
    block_on(sensor.check_who_am_i_async()).unwrap();
    assert_eq!(block_on(sensor.read_config_async()).unwrap(), c);
    block_on(sensor.power_down_async()).unwrap();
    block_on(sensor.set_block_data_update_async(true)).unwrap();
    block_on(sensor.trigger_single_conversion_async()).unwrap();
    assert_eq!(block_on(sensor.status_async()).unwrap(), s);
    block_on(sensor.configure_async(&config)).unwrap();
    assert_eq!(block_on(sensor.read_acceleration_async()).unwrap(), a);
    assert_eq!(
        block_on(sensor.read_acceleration_mg_async(&config)).unwrap(),
        m
    );
    assert_eq!(block_on(sensor.read_temperature_raw_async()).unwrap(), t);
    assert_eq!(
        block_on(sensor.read_temperature_celsius_async()).unwrap(),
        tc
    );
    block_on(sensor.set_interrupt_pin_config_async(&pins)).unwrap();
    assert_eq!(block_on(sensor.interrupt_pin_config_async()).unwrap(), p);
    block_on(sensor.enable_wake_up_interrupt_async(&wake)).unwrap();
    block_on(sensor.disable_wake_up_interrupt_async()).unwrap();
    assert_eq!(block_on(sensor.read_wake_up_source_async()).unwrap(), w);
    assert_eq!(block_on(sensor.clear_interrupts_async()).unwrap(), i);
    block_on(sensor.write_register_async(0x33, 0x5A)).unwrap();
    assert_eq!(block_on(sensor.who_am_i_async()).unwrap(), who);
    let async_i2c = sensor.release();

    assert_eq!(blocking_i2c.events, async_i2c.events);
    assert_eq!(delay.delayed_ms, async_delay.delayed_ms);
    assert!(async_i2c.reads.is_empty());
}

#[cfg(feature = "async")]
#[test]
fn async_init_and_invalid_config_match_blocking() {
    let mut delay = MockAsyncDelay::default();
    let mut sensor = sensor([0x04, 0x33]);
    assert_eq!(
        block_on(sensor.init_async(&mut delay)),
        Err(Error::UnexpectedChipId(0x33))
    );
    let mut sensor = Lis2dw12::new(MockI2c::default());
    let bad = WakeUpConfig {
        threshold: 64,
        duration: 0,
    };
    assert_eq!(
        block_on(sensor.enable_wake_up_interrupt_async(&bad)),
        Err(Error::InvalidConfig)
    );
    assert!(sensor.release().events.is_empty());
}
