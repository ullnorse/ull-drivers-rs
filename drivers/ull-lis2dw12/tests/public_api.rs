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
use ull_lis2dw12::{
    Address, Config, Error, FullScale, InterruptMode, InterruptOutput, InterruptPinConfig,
    InterruptPolarity, Lis2dw12, OperatingMode, OutputDataRate, WakeUpConfig,
};

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

#[test]
fn default_address_is_0x19_and_alternate_is_0x18() {
    let sensor = Lis2dw12::new(MockI2c::default());
    assert_eq!(sensor.address(), 0x19);
    let sensor = Lis2dw12::with_address(MockI2c::default(), Address::ALTERNATE);
    assert_eq!(sensor.address(), 0x18);
}

#[test]
fn start_up_and_read_flow() {
    let mut sensor = Lis2dw12::new(MockI2c::new([
        Vec::from([0x04]),
        Vec::from([0x44]),
        Vec::from([0x00, 0x40, 0x00, 0xC0, 0x04, 0x00]),
    ]));
    let mut delay = MockDelay::default();
    let config = Config {
        odr: OutputDataRate::Hz50,
        mode: OperatingMode::HighPerformance,
        full_scale: FullScale::G2,
        ..Config::default()
    };

    sensor.init(&mut delay).unwrap();
    sensor.configure(&config).unwrap();
    let mg = sensor.read_acceleration_mg(&config).unwrap();
    let i2c = sensor.release();

    assert!((mg[0] - 999.424).abs() < 0.001);
    assert!(i2c.writes().contains(&Vec::from([0x25, 0x00])));
    assert!(i2c.writes().contains(&Vec::from([0x20, 0x44])));
    assert!(i2c.events.iter().all(|event| matches!(
        event,
        Event::Write { address: 0x19, .. } | Event::Read { address: 0x19, .. }
    )));
}

#[test]
fn wake_on_motion_setup_for_an_active_low_latched_pin() {
    let mut sensor = Lis2dw12::new(MockI2c::new([
        Vec::from([0x00]), // CTRL3
        Vec::from([0x00]), // CTRL4
        Vec::from([0x00]), // CTRL7
        Vec::from([0x00]), // ALL_INT_SRC
    ]));
    let pins = InterruptPinConfig {
        polarity: InterruptPolarity::ActiveLow,
        output: InterruptOutput::PushPull,
        mode: InterruptMode::Latched,
    };
    let wake = WakeUpConfig::from_milli_g(FullScale::G2, 500, 1).unwrap();

    sensor.set_interrupt_pin_config(&pins).unwrap();
    sensor.enable_wake_up_interrupt(&wake).unwrap();

    assert_eq!(
        sensor.release().writes(),
        Vec::from([
            Vec::from([0x22, 0x18]),
            Vec::from([0x34, 0x10]),
            Vec::from([0x35, 0x20]),
            Vec::from([0x23, 0x20]),
            Vec::from([0x3F, 0x20]),
        ])
    );
}

#[test]
fn errors_are_visible_to_callers() {
    let mut sensor = Lis2dw12::new(MockI2c::new([Vec::from([0x00])]));
    assert!(matches!(
        sensor.check_who_am_i(),
        Err(Error::UnexpectedChipId(0x00))
    ));
    let mut sensor = Lis2dw12::new(MockI2c::failing_writes());
    assert!(matches!(sensor.write_register(0x20, 0), Err(Error::Bus(_))));
}

#[cfg(feature = "async")]
#[test]
fn async_flow_matches_blocking() {
    let reads = || {
        [
            Vec::from([0x04]),
            Vec::from([0x44]),
            Vec::from([0x00, 0x40, 0x00, 0xC0, 0x04, 0x00]),
        ]
    };
    let mut blocking = Lis2dw12::new(MockI2c::new(reads()));
    let mut delay = MockDelay::default();
    blocking.init(&mut delay).unwrap();
    let a = blocking.read_acceleration().unwrap();

    let mut sensor = Lis2dw12::new(MockI2c::new(reads()));
    let mut async_delay = MockAsyncDelay::default();
    block_on(sensor.init_async(&mut async_delay)).unwrap();
    assert_eq!(block_on(sensor.read_acceleration_async()).unwrap(), a);
    assert_eq!(blocking.release().events, sensor.release().events);
    assert_eq!(delay.delayed_ms, async_delay.delayed_ms);
}
