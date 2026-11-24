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
use ull_bmp580::{
    Address, Bmp580, Config, Error, InterruptConfig, InterruptMode, InterruptOutput,
    InterruptPolarity, OutputDataRate, Oversampling, PowerMode,
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

#[test]
fn forced_measurement_flow_on_default_address() {
    let data = Vec::from([0x00, 0x00, 0x19, 0x40, 0xF3, 0x62]);
    let mut sensor = Bmp580::new(MockI2c::new([
        Vec::from([0x50]),
        Vec::from([0x02]),
        Vec::from([0x10]),
        Vec::from([0x70]),
        Vec::from([0x50, 0x70]),
        data,
    ]));
    let mut delay = MockDelay::default();
    assert_eq!(sensor.address(), 0x46);

    sensor.init(&mut delay).unwrap();
    sensor.configure(&mut delay, &Config::default()).unwrap();
    let measurement = sensor.measure_forced(&mut delay).unwrap().to_fixed_point();
    let i2c = sensor.release();

    assert_eq!(measurement.temperature_millicelsius, 25_000);
    assert_eq!(measurement.pressure_millipascal, 101_325_000);
    assert!(i2c.writes().contains(&Vec::from([0x37, 0x72])));
    assert!(i2c.events.iter().all(|event| matches!(
        event,
        Event::Write { address: 0x46, .. } | Event::Read { address: 0x46, .. }
    )));
}

#[test]
fn alternate_address_is_used_on_the_wire() {
    let mut sensor = Bmp580::with_address(MockI2c::new([Vec::from([0x50])]), Address::ALTERNATE);
    sensor.check_chip_id().unwrap();
    assert!(matches!(
        sensor.release().events[0],
        Event::Write { address: 0x47, .. }
    ));
}

#[test]
fn interrupt_default_keeps_open_drain_active_low() {
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x00])]));
    sensor
        .configure_data_ready_interrupt(&InterruptConfig::default())
        .unwrap();
    // Reset value 0x35 plus int_en: open drain and active low are untouched.
    assert_eq!(sensor.release().writes()[2], Vec::from([0x14, 0x3D]));
    assert_eq!(
        InterruptConfig::default().output,
        InterruptOutput::OpenDrain
    );
    assert_eq!(
        InterruptConfig::default().polarity,
        InterruptPolarity::ActiveLow
    );
    assert_eq!(InterruptConfig::default().mode, InterruptMode::Latched);
}

#[test]
fn invalid_config_and_wrong_chip_are_errors() {
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x51])]));
    assert_eq!(sensor.check_chip_id(), Err(Error::UnexpectedChipId(0x51)));
    let bad = InterruptConfig {
        output: InterruptOutput::PushPull,
        drive_strength: 15,
        ..InterruptConfig::default()
    };
    assert_eq!(
        sensor.set_interrupt_config(&bad, true),
        Err(Error::InvalidConfig)
    );
}

#[test]
fn normal_mode_with_odr_and_raw_registers() {
    let config = Config {
        odr: OutputDataRate::Hz10,
        osr_pressure: Oversampling::X16,
        ..Config::default()
    };
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x70]), Vec::from([0x5C])]));
    sensor
        .configure(&mut MockDelay::default(), &config)
        .unwrap();
    sensor.set_power_mode(PowerMode::Normal).unwrap();
    sensor.write_register(0x34, 0x32).unwrap();
    let writes = sensor.release().writes();
    assert_eq!(writes[3], Vec::from([0x37, 0x5C]));
    assert_eq!(writes[5], Vec::from([0x37, 0x5D]));
    assert_eq!(writes[6], Vec::from([0x34, 0x32]));
}

#[cfg(feature = "async")]
#[test]
fn async_smoke() {
    let mut sensor = Bmp580::new(MockI2c::new([Vec::from([0x50, 0x70]), Vec::from([0; 6])]));
    let mut delay = MockAsyncDelay::default();
    let raw = block_on(sensor.measure_forced_async(&mut delay)).unwrap();
    assert_eq!(raw.temperature_millicelsius(), 0);
    assert_eq!(delay.delayed_ms, Vec::from([9]));
}
