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
use ull_sht3x::{
    Address, Error, FixedPointMeasurement, Measurement, PeriodicRate, RawMeasurement,
    Repeatability, Sht3x, crc8,
};

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct MockI2cError(ErrorKind);

impl MockI2cError {
    const fn no_ack_read_header() -> Self {
        Self(ErrorKind::NoAcknowledge(NoAcknowledgeSource::Address))
    }
}

impl embedded_hal::i2c::Error for MockI2cError {
    fn kind(&self) -> ErrorKind {
        self.0
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Write {
    address: u8,
    bytes: Vec<u8>,
}

#[derive(Debug)]
enum ReadResponse {
    Data(Vec<u8>),
    Error(MockI2cError),
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
    ) -> Result<(), Self::Error> {
        for operation in operations {
            match operation {
                Operation::Read(buffer) => match self.reads.remove(0) {
                    ReadResponse::Data(data) => {
                        assert_eq!(buffer.len(), data.len());
                        buffer.copy_from_slice(&data);
                    }
                    ReadResponse::Error(error) => return Err(error),
                },
                Operation::Write(bytes) => self.writes.push(Write {
                    address,
                    bytes: bytes.to_vec(),
                }),
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
    ) -> Result<(), Self::Error> {
        for operation in operations {
            match operation {
                embedded_hal_async::i2c::Operation::Read(buffer) => match self.reads.remove(0) {
                    ReadResponse::Data(data) => {
                        assert_eq!(buffer.len(), data.len());
                        buffer.copy_from_slice(&data);
                    }
                    ReadResponse::Error(error) => return Err(error),
                },
                embedded_hal_async::i2c::Operation::Write(bytes) => self.writes.push(Write {
                    address,
                    bytes: bytes.to_vec(),
                }),
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

#[test]
fn blocking_single_measurement_uses_public_api() {
    let i2c = MockI2c::new([measurement_bytes(0x6666, 0x6666)]);
    let mut sensor = Sht3x::with_address(i2c, Address::ALTERNATE);
    let mut delay = MockDelay::default();

    let measurement = sensor.measure(&mut delay, Repeatability::High).unwrap();
    let i2c = sensor.release();

    assert_measurement_close(measurement, 25.0, 40.0);
    assert_eq!(delay.delayed_ms, Vec::from([15]));
    assert_eq!(
        i2c.writes,
        Vec::from([Write {
            address: 0x45,
            bytes: Vec::from([0x24, 0x00]),
        }])
    );
}

#[test]
fn raw_measurement_supports_fixed_point_conversion() {
    let i2c = MockI2c::new([measurement_bytes(0x6666, 0x6666)]);
    let mut sensor = Sht3x::new(i2c);
    let mut delay = MockDelay::default();

    let raw = sensor
        .measure_raw(&mut delay, Repeatability::Medium)
        .unwrap();
    let i2c = sensor.release();

    assert_eq!(
        raw,
        RawMeasurement {
            temperature: 0x6666,
            humidity: 0x6666,
        }
    );
    assert_eq!(
        raw.to_fixed_point(),
        FixedPointMeasurement {
            temperature_millicelsius: 25_000,
            relative_humidity_hundredths: 4_000,
        }
    );
    assert_eq!(delay.delayed_ms, Vec::from([6]));
    assert_eq!(
        i2c.writes,
        Vec::from([Write {
            address: 0x44,
            bytes: Vec::from([0x24, 0x0B]),
        }])
    );
}

#[test]
fn periodic_fetch_maps_read_header_nack_to_not_ready() {
    let i2c =
        MockI2c::with_read_responses([ReadResponse::Error(MockI2cError::no_ack_read_header())]);
    let sensor = Sht3x::new(i2c);
    let mut sensor = sensor
        .start_periodic(Repeatability::Low, PeriodicRate::Mps1)
        .unwrap();

    assert_eq!(sensor.fetch(), Err(Error::NotReady));

    let i2c = sensor.release();
    assert_eq!(
        i2c.writes,
        Vec::from([
            Write {
                address: 0x44,
                bytes: Vec::from([0x21, 0x2D]),
            },
            Write {
                address: 0x44,
                bytes: Vec::from([0xE0, 0x00]),
            },
        ])
    );
}

#[test]
fn periodic_mode_returns_single_shot_driver_after_stop() {
    let i2c = MockI2c::new([
        measurement_bytes(0x6666, 0x6666),
        measurement_bytes(0x6666, 0x6666),
    ]);
    let mut delay = MockDelay::default();

    let mut sensor = Sht3x::new(i2c)
        .start_periodic_and_wait(&mut delay, Repeatability::Low, PeriodicRate::Mps1)
        .unwrap();
    let periodic = sensor.fetch().unwrap();
    let mut sensor = sensor.stop_periodic(&mut delay).unwrap();
    let single_shot = sensor.measure(&mut delay, Repeatability::Low).unwrap();
    let i2c = sensor.release();

    assert_measurement_close(periodic, 25.0, 40.0);
    assert_measurement_close(single_shot, 25.0, 40.0);
    assert_eq!(delay.delayed_ms, Vec::from([1, 1, 4]));
    assert_eq!(
        i2c.writes,
        Vec::from([
            Write {
                address: 0x44,
                bytes: Vec::from([0x21, 0x2D]),
            },
            Write {
                address: 0x44,
                bytes: Vec::from([0xE0, 0x00]),
            },
            Write {
                address: 0x44,
                bytes: Vec::from([0x30, 0x93]),
            },
            Write {
                address: 0x44,
                bytes: Vec::from([0x24, 0x16]),
            },
        ])
    );
}

#[cfg(feature = "async")]
#[test]
fn async_measurement_smoke_test() {
    let i2c = MockI2c::new([measurement_bytes(0x6666, 0x6666)]);
    let mut sensor = Sht3x::new(i2c);
    let mut delay = MockAsyncDelay::default();

    let measurement = block_on(sensor.measure_async(&mut delay, Repeatability::High)).unwrap();
    let i2c = sensor.release();

    assert_measurement_close(measurement, 25.0, 40.0);
    assert_eq!(delay.delayed_ms, Vec::from([15]));
    assert_eq!(
        i2c.writes,
        Vec::from([Write {
            address: 0x44,
            bytes: Vec::from([0x24, 0x00]),
        }])
    );
}

fn assert_measurement_close(
    measurement: Measurement,
    temperature_celsius: f32,
    relative_humidity: f32,
) {
    assert!((measurement.temperature_celsius - temperature_celsius).abs() < 0.001);
    assert!((measurement.relative_humidity - relative_humidity).abs() < 0.001);
}
