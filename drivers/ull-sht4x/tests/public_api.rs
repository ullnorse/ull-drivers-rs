#![allow(dead_code)]

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
use ull_sht4x::{Address, Error, HeaterDuration, HeaterPower, Repeatability, Sht4x, crc8};

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
fn blocking_flow_with_alternate_address() {
    let mut sensor = Sht4x::with_address(
        MockI2c::new([measurement_bytes(0x6666, 0x6666)]),
        Address::ALTERNATE,
    );
    let mut delay = MockDelay::default();

    let fixed = sensor
        .measure_raw(&mut delay, Repeatability::High)
        .unwrap()
        .to_fixed_point();
    let i2c = sensor.release();

    assert_eq!(fixed.temperature_millicelsius, 25_000);
    assert_eq!(fixed.relative_humidity_hundredths, 4_400);
    assert_eq!(
        i2c.events,
        Vec::from([
            Event::Write {
                address: 0x45,
                bytes: Vec::from([0xFD])
            },
            Event::Read {
                address: 0x45,
                len: 6
            },
        ])
    );
}

#[test]
fn default_address_and_serial_number() {
    let mut bytes = word_bytes(0xDEAD);
    bytes.extend(word_bytes(0xBEEF));
    let mut sensor = Sht4x::new(MockI2c::new([bytes]));
    assert_eq!(sensor.address(), 0x44);
    assert_eq!(sensor.serial_number().unwrap(), 0xDEAD_BEEF);
}

#[test]
fn bad_crc_is_reported() {
    let mut bytes = measurement_bytes(1, 2);
    bytes[2] ^= 1;
    let mut sensor = Sht4x::new(MockI2c::new([bytes]));
    assert!(matches!(
        sensor.measure(&mut MockDelay::default(), Repeatability::Low),
        Err(Error::Crc { .. })
    ));
}

#[test]
fn heater_raw_escape_hatch() {
    let mut sensor = Sht4x::new(MockI2c::new([measurement_bytes(1, 2)]));
    let raw = sensor
        .measure_with_heater_raw(
            &mut MockDelay::default(),
            HeaterPower::Mw200,
            HeaterDuration::Short,
        )
        .unwrap();
    assert_eq!(raw.temperature, 1);
    assert_eq!(sensor.release().writes(), Vec::from([Vec::from([0x32])]));
}

#[cfg(feature = "async")]
#[test]
fn async_smoke() {
    let mut sensor = Sht4x::new(MockI2c::new([measurement_bytes(0x6666, 0x6666)]));
    let mut delay = MockAsyncDelay::default();
    let measurement = block_on(sensor.measure_async(&mut delay, Repeatability::Medium)).unwrap();
    assert!((measurement.temperature_celsius - 25.0).abs() < 1e-3);
    assert_eq!(delay.delayed_ms, Vec::from([5]));
}
