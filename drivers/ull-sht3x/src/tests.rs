extern crate std;

use core::convert::Infallible;
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

use crate::types::parse_raw_measurement;
use crate::*;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct MockI2cError(ErrorKind);

impl MockI2cError {
    const fn other() -> Self {
        Self(ErrorKind::Other)
    }

    const fn no_ack_data() -> Self {
        Self(ErrorKind::NoAcknowledge(NoAcknowledgeSource::Data))
    }
}

impl embedded_hal::i2c::Error for MockI2cError {
    fn kind(&self) -> ErrorKind {
        self.0
    }
}

#[derive(Debug)]
enum ReadResponse {
    Data(Vec<u8>),
    Error(MockI2cError),
}

#[derive(Debug)]
struct Write {
    address: u8,
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct MockI2c {
    writes: Vec<Write>,
    reads: Vec<ReadResponse>,
}

impl MockI2c {
    fn new(reads: impl IntoIterator<Item = Vec<u8>>) -> Self {
        Self {
            writes: Vec::new(),
            reads: reads.into_iter().map(ReadResponse::Data).collect(),
        }
    }

    fn with_read_responses(reads: impl IntoIterator<Item = ReadResponse>) -> Self {
        Self {
            writes: Vec::new(),
            reads: reads.into_iter().collect(),
        }
    }
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
                Operation::Read(buffer) => match self.reads.remove(0) {
                    ReadResponse::Data(read) => buffer.copy_from_slice(&read),
                    ReadResponse::Error(error) => return Err(error),
                },
                Operation::Write(bytes) => {
                    self.writes.push(Write {
                        address,
                        bytes: bytes.to_vec(),
                    });
                }
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
                embedded_hal_async::i2c::Operation::Read(buffer) => match self.reads.remove(0) {
                    ReadResponse::Data(read) => buffer.copy_from_slice(&read),
                    ReadResponse::Error(error) => return Err(error),
                },
                embedded_hal_async::i2c::Operation::Write(bytes) => {
                    self.writes.push(Write {
                        address,
                        bytes: bytes.to_vec(),
                    });
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

fn measurement_bytes(temperature: u16, humidity: u16) -> Vec<u8> {
    let [t_msb, t_lsb] = temperature.to_be_bytes();
    let [h_msb, h_lsb] = humidity.to_be_bytes();
    Vec::from([
        t_msb,
        t_lsb,
        crc8([t_msb, t_lsb]),
        h_msb,
        h_lsb,
        crc8([h_msb, h_lsb]),
    ])
}

fn temperature_bytes(temperature: u16) -> Vec<u8> {
    let [msb, lsb] = temperature.to_be_bytes();
    Vec::from([msb, lsb, crc8([msb, lsb])])
}

#[test]
fn crc_matches_datasheet_example() {
    assert_eq!(crc8([0xBE, 0xEF]), 0x92);
}

#[test]
fn parses_raw_measurement_and_converts_units() {
    let raw =
        parse_raw_measurement::<Infallible>(measurement_bytes(0, 0).try_into().unwrap()).unwrap();

    assert_eq!(raw.temperature, 0);
    assert_eq!(raw.humidity, 0);
    assert_eq!(raw.temperature_celsius(), -45.0);
    assert_eq!(raw.relative_humidity(), 0.0);

    let raw =
        parse_raw_measurement::<Infallible>(measurement_bytes(0xFFFF, 0xFFFF).try_into().unwrap())
            .unwrap();

    assert_eq!(raw.temperature_celsius(), 130.0);
    assert!((raw.temperature_fahrenheit() - 266.0).abs() < 0.001);
    assert_eq!(raw.relative_humidity(), 100.0);
}

#[test]
fn converts_units_with_integer_only_math() {
    let raw = RawMeasurement {
        temperature: 0,
        humidity: 0,
    };

    assert_eq!(raw.temperature_millicelsius(), -45_000);
    assert_eq!(raw.temperature_millifahrenheit(), -49_000);
    assert_eq!(raw.relative_humidity_hundredths(), 0);
    assert_eq!(
        raw.to_fixed_point(),
        FixedPointMeasurement {
            temperature_millicelsius: -45_000,
            relative_humidity_hundredths: 0,
        }
    );

    let raw = RawMeasurement {
        temperature: 0xFFFF,
        humidity: 0xFFFF,
    };

    assert_eq!(raw.temperature_millicelsius(), 130_000);
    assert_eq!(raw.temperature_millifahrenheit(), 266_000);
    assert_eq!(raw.relative_humidity_hundredths(), 10_000);
}

#[test]
fn rejects_bad_temperature_crc() {
    let mut bytes = measurement_bytes(0x1234, 0x5678);
    bytes[2] ^= 0x01;

    assert_eq!(
        parse_raw_measurement::<Infallible>(bytes.try_into().unwrap()),
        Err(Error::Crc {
            word: DataWord::Temperature,
            expected: crc8([0x12, 0x34]),
            actual: crc8([0x12, 0x34]) ^ 0x01,
        })
    );
}

#[test]
fn rejects_bad_humidity_crc() {
    let mut bytes = measurement_bytes(0x1234, 0x5678);
    bytes[5] ^= 0x01;

    assert_eq!(
        parse_raw_measurement::<Infallible>(bytes.try_into().unwrap()),
        Err(Error::Crc {
            word: DataWord::Humidity,
            expected: crc8([0x56, 0x78]),
            actual: crc8([0x56, 0x78]) ^ 0x01,
        })
    );
}

#[test]
fn rejects_bad_status_crc() {
    let status = 0x1234u16;
    let [msb, lsb] = status.to_be_bytes();
    let i2c = MockI2c::with_read_responses([ReadResponse::Data(Vec::from([
        msb,
        lsb,
        crc8([msb, lsb]) ^ 0x01,
    ]))]);
    let mut sensor = Sht3x::new(i2c);

    assert_eq!(
        sensor.status(),
        Err(Error::Crc {
            word: DataWord::Status,
            expected: crc8([msb, lsb]),
            actual: crc8([msb, lsb]) ^ 0x01,
        })
    );
}

#[test]
fn formats_crc_errors_for_logs() {
    let error = Error::<Infallible>::Crc {
        word: DataWord::Humidity,
        expected: 0x92,
        actual: 0x93,
    };

    assert_eq!(
        std::format!("{error}"),
        "CRC mismatch for Humidity: expected 0x92, got 0x93"
    );
}

#[test]
fn measures_single_shot_high_repeatability_without_clock_stretching() {
    let i2c = MockI2c::new([measurement_bytes(0x6666, 0x8000)]);
    let mut sensor = Sht3x::new(i2c);
    let mut delay = MockDelay::default();

    let raw = sensor.measure_raw(&mut delay, Repeatability::High).unwrap();
    let i2c = sensor.release();

    assert_eq!(raw.temperature, 0x6666);
    assert_eq!(raw.humidity, 0x8000);
    assert_eq!(delay.delayed_ms, Vec::from([15]));
    assert_eq!(i2c.writes.len(), 1);
    assert_eq!(i2c.writes[0].address, 0x44);
    assert_eq!(i2c.writes[0].bytes, Vec::from([0x24, 0x00]));
}

#[test]
fn measures_temperature_only_with_short_read() {
    let i2c = MockI2c::new([temperature_bytes(0x6666)]);
    let mut sensor = Sht3x::new(i2c);
    let mut delay = MockDelay::default();

    let raw = sensor
        .measure_temperature_raw(&mut delay, Repeatability::High)
        .unwrap();
    let i2c = sensor.release();

    assert_eq!(raw, 0x6666);
    assert_eq!(delay.delayed_ms, Vec::from([15]));
    assert_eq!(i2c.writes[0].bytes, Vec::from([0x24, 0x00]));
}

#[test]
fn low_voltage_measurement_uses_longer_delay() {
    let i2c = MockI2c::new([measurement_bytes(0x6666, 0x8000)]);
    let mut sensor = Sht3x::new(i2c);
    let mut delay = MockDelay::default();

    sensor
        .measure_raw_low_voltage(&mut delay, Repeatability::Medium)
        .unwrap();

    assert_eq!(delay.delayed_ms, Vec::from([7]));
}

#[test]
fn repeatability_delay_tables_match_datasheet_maxima() {
    assert_eq!(Repeatability::Low.delay_ms(), 4);
    assert_eq!(Repeatability::Medium.delay_ms(), 6);
    assert_eq!(Repeatability::High.delay_ms(), 15);

    assert_eq!(Repeatability::Low.low_voltage_delay_ms(), 5);
    assert_eq!(Repeatability::Medium.low_voltage_delay_ms(), 7);
    assert_eq!(Repeatability::High.low_voltage_delay_ms(), 16);
}

#[test]
fn periodic_commands_match_datasheet_table() {
    let expected = [
        (PeriodicRate::Mps0_5, Repeatability::High, 0x2032),
        (PeriodicRate::Mps0_5, Repeatability::Medium, 0x2024),
        (PeriodicRate::Mps0_5, Repeatability::Low, 0x202F),
        (PeriodicRate::Mps1, Repeatability::High, 0x2130),
        (PeriodicRate::Mps1, Repeatability::Medium, 0x2126),
        (PeriodicRate::Mps1, Repeatability::Low, 0x212D),
        (PeriodicRate::Mps2, Repeatability::High, 0x2236),
        (PeriodicRate::Mps2, Repeatability::Medium, 0x2220),
        (PeriodicRate::Mps2, Repeatability::Low, 0x222B),
        (PeriodicRate::Mps4, Repeatability::High, 0x2334),
        (PeriodicRate::Mps4, Repeatability::Medium, 0x2322),
        (PeriodicRate::Mps4, Repeatability::Low, 0x2329),
        (PeriodicRate::Mps10, Repeatability::High, 0x2737),
        (PeriodicRate::Mps10, Repeatability::Medium, 0x2721),
        (PeriodicRate::Mps10, Repeatability::Low, 0x272A),
    ];

    for (rate, repeatability, command) in expected {
        assert_eq!(rate.command(repeatability), command);
    }
}

#[test]
fn uses_alternate_address_and_periodic_commands() {
    let i2c = MockI2c::new([measurement_bytes(0x1111, 0x2222)]);
    let mut sensor = Sht3x::with_address(i2c, Address::ALTERNATE);

    sensor
        .start_periodic(Repeatability::Medium, PeriodicRate::Mps4)
        .unwrap();
    let raw = sensor.fetch_raw().unwrap();
    let i2c = sensor.release();

    assert_eq!(raw.temperature, 0x1111);
    assert_eq!(raw.humidity, 0x2222);
    assert_eq!(i2c.writes[0].address, 0x45);
    assert_eq!(i2c.writes[0].bytes, Vec::from([0x23, 0x22]));
    assert_eq!(i2c.writes[1].bytes, Vec::from([0xE0, 0x00]));
}

#[test]
fn fetch_maps_not_ready_nack() {
    let i2c = MockI2c::with_read_responses([ReadResponse::Error(MockI2cError::no_ack_data())]);
    let mut sensor = Sht3x::new(i2c);

    assert_eq!(sensor.fetch_raw(), Err(Error::NotReady));

    let i2c = sensor.release();
    assert_eq!(i2c.writes[0].bytes, Vec::from([0xE0, 0x00]));
}

#[test]
fn fetch_propagates_other_i2c_error() {
    let i2c = MockI2c::with_read_responses([ReadResponse::Error(MockI2cError::other())]);
    let mut sensor = Sht3x::new(i2c);

    assert_eq!(sensor.fetch_raw(), Err(Error::I2c(MockI2cError::other())));
}

#[test]
fn configuration_wait_variants_enforce_command_gap() {
    let i2c = MockI2c::new([]);
    let mut sensor = Sht3x::new(i2c);
    let mut delay = MockDelay::default();

    sensor.clear_status_and_wait(&mut delay).unwrap();
    sensor.set_heater_and_wait(&mut delay, true).unwrap();
    sensor.start_art_and_wait(&mut delay).unwrap();
    sensor
        .start_periodic_and_wait(&mut delay, Repeatability::Low, PeriodicRate::Mps1)
        .unwrap();
    let i2c = sensor.release();

    assert_eq!(delay.delayed_ms, Vec::from([1, 1, 1, 1]));
    assert_eq!(i2c.writes[0].bytes, Vec::from([0x30, 0x41]));
    assert_eq!(i2c.writes[1].bytes, Vec::from([0x30, 0x6D]));
    assert_eq!(i2c.writes[2].bytes, Vec::from([0x2B, 0x32]));
    assert_eq!(i2c.writes[3].bytes, Vec::from([0x21, 0x2D]));
}

#[test]
fn reads_status_and_exposes_bits() {
    let status = 0b1010_0000_0001_0011u16;
    let [msb, lsb] = status.to_be_bytes();
    let i2c = MockI2c::new([Vec::from([msb, lsb, crc8([msb, lsb])])]);
    let mut sensor = Sht3x::new(i2c);

    let status = sensor.status().unwrap();

    assert!(status.alert_pending());
    assert!(status.heater_enabled());
    assert!(status.reset_detected());
    assert!(status.command_failed());
    assert!(status.write_checksum_failed());
    assert!(!status.humidity_alert());
}

#[test]
fn sends_general_call_reset_to_address_zero() {
    let i2c = MockI2c::new([]);
    let mut sensor = Sht3x::new(i2c);
    let mut delay = MockDelay::default();

    sensor.general_call_reset(&mut delay).unwrap();
    let i2c = sensor.release();

    assert_eq!(i2c.writes[0].address, 0x00);
    assert_eq!(i2c.writes[0].bytes, Vec::from([0x06]));
    assert_eq!(delay.delayed_ms, Vec::from([2]));
}

#[cfg(feature = "async")]
#[test]
fn async_measure_uses_single_shot_delay_and_reads_measurement() {
    let i2c = MockI2c::new([measurement_bytes(0x6666, 0x8000)]);
    let mut sensor = Sht3x::new(i2c);
    let mut delay = MockAsyncDelay::default();

    let raw = block_on(sensor.measure_raw_async(&mut delay, Repeatability::High)).unwrap();
    let i2c = sensor.release();

    assert_eq!(raw.temperature, 0x6666);
    assert_eq!(raw.humidity, 0x8000);
    assert_eq!(delay.delayed_ms, Vec::from([15]));
    assert_eq!(i2c.writes[0].bytes, Vec::from([0x24, 0x00]));
}

#[cfg(feature = "async")]
#[test]
fn async_periodic_commands_match_sync_sequence() {
    let i2c = MockI2c::new([measurement_bytes(0x1111, 0x2222)]);
    let mut sensor = Sht3x::with_address(i2c, Address::ALTERNATE);
    let mut delay = MockAsyncDelay::default();

    block_on(sensor.start_periodic_and_wait_async(
        &mut delay,
        Repeatability::Medium,
        PeriodicRate::Mps4,
    ))
    .unwrap();
    let raw = block_on(sensor.fetch_raw_async()).unwrap();
    let i2c = sensor.release();

    assert_eq!(raw.temperature, 0x1111);
    assert_eq!(raw.humidity, 0x2222);
    assert_eq!(delay.delayed_ms, Vec::from([1]));
    assert_eq!(i2c.writes[0].address, 0x45);
    assert_eq!(i2c.writes[0].bytes, Vec::from([0x23, 0x22]));
    assert_eq!(i2c.writes[1].bytes, Vec::from([0xE0, 0x00]));
}

#[cfg(feature = "async")]
#[test]
fn async_general_call_reset_waits_and_uses_address_zero() {
    let i2c = MockI2c::new([]);
    let mut sensor = Sht3x::new(i2c);
    let mut delay = MockAsyncDelay::default();

    block_on(sensor.general_call_reset_async(&mut delay)).unwrap();
    let i2c = sensor.release();

    assert_eq!(i2c.writes[0].address, 0x00);
    assert_eq!(i2c.writes[0].bytes, Vec::from([0x06]));
    assert_eq!(delay.delayed_ms, Vec::from([2]));
}

#[cfg(feature = "async")]
#[test]
fn async_status_reads_and_validates_crc() {
    let status = 0b1010_0000_0001_0011u16;
    let [msb, lsb] = status.to_be_bytes();
    let i2c = MockI2c::new([Vec::from([msb, lsb, crc8([msb, lsb])])]);
    let mut sensor = Sht3x::new(i2c);

    let status = block_on(sensor.status_async()).unwrap();

    assert!(status.alert_pending());
    assert!(status.heater_enabled());
    assert!(status.reset_detected());
    assert!(status.command_failed());
    assert!(status.write_checksum_failed());
    assert!(!status.humidity_alert());
}

#[cfg(feature = "async")]
#[test]
fn async_fetch_maps_not_ready_nack() {
    let i2c = MockI2c::with_read_responses([ReadResponse::Error(MockI2cError::no_ack_data())]);
    let mut sensor = Sht3x::new(i2c);

    assert_eq!(block_on(sensor.fetch_raw_async()), Err(Error::NotReady));

    let i2c = sensor.release();
    assert_eq!(i2c.writes[0].bytes, Vec::from([0xE0, 0x00]));
}

#[cfg(feature = "async")]
#[test]
fn async_fetch_propagates_other_i2c_error() {
    let i2c = MockI2c::with_read_responses([ReadResponse::Error(MockI2cError::other())]);
    let mut sensor = Sht3x::new(i2c);

    assert_eq!(
        block_on(sensor.fetch_raw_async()),
        Err(Error::I2c(MockI2cError::other()))
    );
}

#[test]
fn reset_and_break_commands_wait_long_enough() {
    let i2c = MockI2c::new([]);
    let mut sensor = Sht3x::new(i2c);
    let mut delay = MockDelay::default();

    sensor.stop_periodic(&mut delay).unwrap();
    sensor.soft_reset(&mut delay).unwrap();

    let i2c = sensor.release();
    assert_eq!(i2c.writes[0].bytes, Vec::from([0x30, 0x93]));
    assert_eq!(i2c.writes[1].bytes, Vec::from([0x30, 0xA2]));
    assert_eq!(delay.delayed_ms, Vec::from([1, 2]));
}

#[test]
fn custom_address_accepts_only_supported_sensor_addresses() {
    assert_eq!(Address::custom(0x44), Some(Address::DEFAULT));
    assert_eq!(Address::custom(0x45), Some(Address::ALTERNATE));
    assert_eq!(Address::custom(0x00), None);
    assert_eq!(Address::custom(0x43), None);
    assert_eq!(Address::custom(0x46), None);
    assert_eq!(Address::custom(0x7F), None);
}

#[test]
fn address_default_is_sensor_default() {
    assert_eq!(Address::default(), Address::DEFAULT);
    assert_eq!(Address::default().as_u8(), 0x44);
}

#[test]
fn driver_can_be_created_from_i2c_directly() {
    let sensor = Sht3x::from(MockI2c::new([]));
    assert_eq!(sensor.address(), 0x44);
}

fn assert_send<T: Send>() {}

#[test]
fn driver_is_send_when_i2c_is_send() {
    assert_send::<Sht3x<MockI2c>>();
}
