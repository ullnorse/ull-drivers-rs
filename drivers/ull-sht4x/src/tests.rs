extern crate std;

use crate::types::{parse_raw_measurement, parse_serial_number};
use crate::*;
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
    fail_writes: bool,
}

#[allow(dead_code)]
impl MockI2c {
    fn new(reads: impl IntoIterator<Item = Vec<u8>>) -> Self {
        Self {
            events: Vec::new(),
            reads: reads.into_iter().map(ReadResponse::Data).collect(),
            fail_writes: false,
        }
    }

    fn with_read_responses(reads: impl IntoIterator<Item = ReadResponse>) -> Self {
        Self {
            events: Vec::new(),
            reads: reads.into_iter().collect(),
            fail_writes: false,
        }
    }

    fn failing_writes() -> Self {
        Self {
            fail_writes: true,
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
                if self.fail_writes {
                    return Err(MockI2cError::other());
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

fn word_bytes(word: u16) -> Vec<u8> {
    let [msb, lsb] = word.to_be_bytes();
    Vec::from([msb, lsb, crc8([msb, lsb])])
}

fn measurement_bytes(temperature: u16, humidity: u16) -> Vec<u8> {
    let mut bytes = word_bytes(temperature);
    bytes.extend(word_bytes(humidity));
    bytes
}

#[test]
fn crc_matches_datasheet_example() {
    // Table 6 (p. 9): CRC(0xBEEF) = 0x92.
    assert_eq!(crc8([0xBE, 0xEF]), 0x92);
}

#[test]
fn converts_endpoints() {
    let zero = RawMeasurement {
        temperature: 0,
        humidity: 0,
    };
    // Equations (1)/(2): T = -45 degC and RH = -6 % (cropped to 0).
    assert_eq!(zero.temperature_millicelsius(), -45_000);
    assert_eq!(zero.relative_humidity_hundredths(), 0);
    assert_eq!(zero.temperature_celsius(), -45.0);
    assert_eq!(zero.relative_humidity(), 0.0);

    let full = RawMeasurement {
        temperature: 0xFFFF,
        humidity: 0xFFFF,
    };
    // T = -45 + 175 = 130 degC, RH = -6 + 125 = 119 % (cropped to 100).
    assert_eq!(full.temperature_millicelsius(), 130_000);
    assert_eq!(full.relative_humidity_hundredths(), 10_000);
    assert_eq!(full.temperature_celsius(), 130.0);
    assert_eq!(full.relative_humidity(), 100.0);
}

#[test]
fn converts_hand_computed_points() {
    // 0x6666 = 26214 = 0.4 * 65535, so T = -45 + 0.4 * 175 = 25 degC and
    // RH = -6 + 0.4 * 125 = 44 %.
    let raw = RawMeasurement {
        temperature: 0x6666,
        humidity: 0x6666,
    };
    assert_eq!(
        raw.to_fixed_point(),
        FixedPointMeasurement {
            temperature_millicelsius: 25_000,
            relative_humidity_hundredths: 4_400,
        }
    );
    let measurement = raw.to_measurement();
    assert!((measurement.temperature_celsius - 25.0).abs() < 1e-3);
    assert!((measurement.relative_humidity - 44.0).abs() < 1e-3);

    // 0x8000 = 32768: T = -45 + 175 * 32768 / 65535 = 42.5013 degC,
    // RH = -6 + 125 * 32768 / 65535 = 56.5009 %.
    let raw = RawMeasurement {
        temperature: 0x8000,
        humidity: 0x8000,
    };
    assert_eq!(raw.temperature_millicelsius(), 42_501);
    assert_eq!(raw.relative_humidity_hundredths(), 5_650);
}

#[test]
fn fixed_point_and_float_agree() {
    for raw in [0u16, 1, 1000, 12345, 40000, 65534, 65535] {
        let value = RawMeasurement {
            temperature: raw,
            humidity: raw,
        };
        let fixed = value.to_fixed_point();
        let float = value.to_measurement();
        assert!(
            (fixed.temperature_millicelsius as f32 / 1000.0 - float.temperature_celsius).abs()
                < 0.002
        );
        assert!(
            (f32::from(fixed.relative_humidity_hundredths) / 100.0 - float.relative_humidity).abs()
                < 0.011
        );
    }
}

#[test]
fn rejects_bad_crc_per_word() {
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
fn formats_errors_for_logs() {
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
fn address_validation() {
    assert_eq!(Address::default().as_u8(), 0x44);
    assert_eq!(Address::ALTERNATE.as_u8(), 0x45);
    assert_eq!(Address::custom(0x45), Some(Address::ALTERNATE));
    assert_eq!(Address::custom(0x46), None);
}

#[test]
fn measure_high_writes_command_waits_and_reads_six_bytes() {
    let mut sensor = Sht4x::new(MockI2c::new([measurement_bytes(0x6666, 0x6666)]));
    let mut delay = MockDelay::default();

    let raw = sensor.measure_raw(&mut delay, Repeatability::High).unwrap();
    let i2c = sensor.release();

    assert_eq!(raw.temperature, 0x6666);
    assert_eq!(delay.delayed_ms, Vec::from([9]));
    assert_eq!(
        i2c.events,
        Vec::from([
            Event::Write {
                address: 0x44,
                bytes: Vec::from([0xFD])
            },
            Event::Read {
                address: 0x44,
                len: 6
            },
        ])
    );
}

#[test]
fn repeatability_commands_and_delays() {
    for (repeatability, command, delay_ms) in [
        (Repeatability::High, 0xFD, 9),
        (Repeatability::Medium, 0xF6, 5),
        (Repeatability::Low, 0xE0, 2),
    ] {
        let mut sensor = Sht4x::new(MockI2c::new([measurement_bytes(0, 0)]));
        let mut delay = MockDelay::default();
        sensor.measure(&mut delay, repeatability).unwrap();
        assert_eq!(sensor.release().writes(), Vec::from([Vec::from([command])]));
        assert_eq!(delay.delayed_ms, Vec::from([delay_ms]));
    }
}

#[test]
fn measure_returns_converted_units() {
    let mut sensor = Sht4x::with_address(
        MockI2c::new([measurement_bytes(0x6666, 0x6666)]),
        Address::ALTERNATE,
    );
    let measurement = sensor
        .measure(&mut MockDelay::default(), Repeatability::Low)
        .unwrap();
    assert!((measurement.temperature_celsius - 25.0).abs() < 1e-3);
    assert_eq!(sensor.address(), 0x45);
}

#[test]
fn measure_rejects_bad_crc() {
    let mut bytes = measurement_bytes(0x1234, 0x5678);
    bytes[5] ^= 0xFF;
    let mut sensor = Sht4x::new(MockI2c::new([bytes]));
    assert!(matches!(
        sensor.measure_raw(&mut MockDelay::default(), Repeatability::High),
        Err(Error::Crc {
            word: DataWord::Humidity,
            ..
        })
    ));
}

#[test]
fn reads_serial_number() {
    let mut bytes = word_bytes(0x1234);
    bytes.extend(word_bytes(0x5678));
    let mut sensor = Sht4x::new(MockI2c::new([bytes]));
    assert_eq!(sensor.serial_number().unwrap(), 0x1234_5678);
    let i2c = sensor.release();
    assert_eq!(i2c.writes(), Vec::from([Vec::from([0x89])]));
}

#[test]
fn serial_number_checks_both_crcs() {
    let mut bytes = word_bytes(0x1234);
    bytes.extend(word_bytes(0x5678));
    bytes[2] ^= 1;
    assert!(matches!(
        parse_serial_number::<Infallible>(bytes.clone().try_into().unwrap()),
        Err(Error::Crc {
            word: DataWord::SerialHigh,
            ..
        })
    ));
    bytes[2] ^= 1;
    bytes[5] ^= 1;
    assert!(matches!(
        parse_serial_number::<Infallible>(bytes.try_into().unwrap()),
        Err(Error::Crc {
            word: DataWord::SerialLow,
            ..
        })
    ));
}

#[test]
fn soft_reset_writes_command_and_waits() {
    let mut sensor = Sht4x::new(MockI2c::default());
    let mut delay = MockDelay::default();
    sensor.soft_reset(&mut delay).unwrap();
    assert_eq!(sensor.release().writes(), Vec::from([Vec::from([0x94])]));
    assert_eq!(delay.delayed_ms, Vec::from([1]));
}

#[test]
fn heater_commands_match_table_7() {
    for (power, duration, command, delay_ms) in [
        (HeaterPower::Mw200, HeaterDuration::Long, 0x39, 1109),
        (HeaterPower::Mw200, HeaterDuration::Short, 0x32, 119),
        (HeaterPower::Mw110, HeaterDuration::Long, 0x2F, 1109),
        (HeaterPower::Mw110, HeaterDuration::Short, 0x24, 119),
        (HeaterPower::Mw20, HeaterDuration::Long, 0x1E, 1109),
        (HeaterPower::Mw20, HeaterDuration::Short, 0x15, 119),
    ] {
        let mut sensor = Sht4x::new(MockI2c::new([measurement_bytes(1, 2)]));
        let mut delay = MockDelay::default();
        let raw = sensor
            .measure_with_heater_raw(&mut delay, power, duration)
            .unwrap();
        assert_eq!((raw.temperature, raw.humidity), (1, 2));
        assert_eq!(sensor.release().writes(), Vec::from([Vec::from([command])]));
        assert_eq!(delay.delayed_ms, Vec::from([delay_ms]));
    }
}

#[test]
fn raw_escape_hatches() {
    let mut sensor = Sht4x::new(MockI2c::new([Vec::from([1, 2, 3, 4, 5, 6])]));
    sensor.write_command(0xAB).unwrap();
    assert_eq!(sensor.read_response_raw().unwrap(), [1, 2, 3, 4, 5, 6]);
    assert_eq!(sensor.release().writes(), Vec::from([Vec::from([0xAB])]));
}

#[test]
fn bus_errors_propagate() {
    let mut sensor = Sht4x::new(MockI2c::failing_writes());
    assert_eq!(
        sensor.measure_raw(&mut MockDelay::default(), Repeatability::High),
        Err(Error::Bus(MockI2cError::other()))
    );

    // Reading before the conversion finished is NACKed by the sensor.
    let mut sensor = Sht4x::new(MockI2c::with_read_responses([ReadResponse::Error(
        MockI2cError::no_ack(),
    )]));
    assert_eq!(
        sensor.measure_raw(&mut MockDelay::default(), Repeatability::High),
        Err(Error::Bus(MockI2cError::no_ack()))
    );
}

fn assert_send<T: Send>() {}

#[test]
fn driver_is_send_when_i2c_is_send() {
    assert_send::<Sht4x<MockI2c>>();
}

#[cfg(feature = "async")]
#[test]
fn blocking_and_async_apis_produce_identical_bus_traffic() {
    fn reads() -> Vec<Vec<u8>> {
        let mut serial = word_bytes(0xAAAA);
        serial.extend(word_bytes(0x5555));
        Vec::from([
            measurement_bytes(0x6666, 0x8000),
            measurement_bytes(0x1234, 0x4321),
            serial,
            measurement_bytes(0x1111, 0x2222),
        ])
    }

    let mut delay = MockDelay::default();
    let mut sensor = Sht4x::new(MockI2c::new(reads()));
    sensor.soft_reset(&mut delay).unwrap();
    let a = sensor.measure_raw(&mut delay, Repeatability::High).unwrap();
    let b = sensor.measure(&mut delay, Repeatability::Low).unwrap();
    let serial = sensor.serial_number().unwrap();
    let c = sensor
        .measure_with_heater_raw(&mut delay, HeaterPower::Mw20, HeaterDuration::Short)
        .unwrap();
    sensor.write_command(0x01).unwrap();
    let blocking_i2c = sensor.release();

    let mut async_delay = MockAsyncDelay::default();
    let mut sensor = Sht4x::new(MockI2c::new(reads()));
    block_on(sensor.soft_reset_async(&mut async_delay)).unwrap();
    assert_eq!(
        block_on(sensor.measure_raw_async(&mut async_delay, Repeatability::High)).unwrap(),
        a
    );
    assert_eq!(
        block_on(sensor.measure_async(&mut async_delay, Repeatability::Low)).unwrap(),
        b
    );
    assert_eq!(block_on(sensor.serial_number_async()).unwrap(), serial);
    assert_eq!(
        block_on(sensor.measure_with_heater_raw_async(
            &mut async_delay,
            HeaterPower::Mw20,
            HeaterDuration::Short
        ))
        .unwrap(),
        c
    );
    block_on(sensor.write_command_async(0x01)).unwrap();
    let async_i2c = sensor.release();

    assert_eq!(blocking_i2c.events, async_i2c.events);
    assert_eq!(delay.delayed_ms, async_delay.delayed_ms);
}
