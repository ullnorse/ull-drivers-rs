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
use ull_max17048::{Address, Error, Max17048, VoltageAlert};

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

fn word(value: u16) -> Vec<u8> {
    Vec::from(value.to_be_bytes())
}

#[test]
fn blocking_flow_reads_battery_state() {
    let mut gauge =
        Max17048::with_address(MockI2c::new([word(0xB900), word(0x3200)]), Address::DEFAULT);
    assert_eq!(gauge.address(), 0x36);

    assert_eq!(gauge.vcell_millivolts().unwrap(), 3_700);
    assert_eq!(gauge.soc_hundredths().unwrap(), 5_000);

    let i2c = gauge.release();
    assert_eq!(
        i2c.events,
        Vec::from([
            Event::Write {
                address: 0x36,
                bytes: Vec::from([0x02])
            },
            Event::Read {
                address: 0x36,
                len: 2
            },
            Event::Write {
                address: 0x36,
                bytes: Vec::from([0x04])
            },
            Event::Read {
                address: 0x36,
                len: 2
            },
        ])
    );
}

#[test]
fn alert_setup_and_clear_before_deep_sleep() {
    // Reads: CONFIG (low-SOC), then STATUS and CONFIG for clear_alert.
    let mut gauge = Max17048::new(MockI2c::new([word(0x971C), word(0x1000), word(0x973C)]));
    gauge.set_low_soc_threshold(15).unwrap();
    gauge
        .set_voltage_alert(VoltageAlert {
            min_mv: 3300,
            max_mv: 4300,
        })
        .unwrap();
    let pending = gauge.clear_alert().unwrap();
    assert!(pending.soc_low());
    let writes = gauge.release().writes();
    assert_eq!(writes[1], Vec::from([0x0C, 0x97, 0x11]));
    assert_eq!(writes[2], Vec::from([0x14, 0xA5, 0xD7]));
    assert_eq!(writes[4], Vec::from([0x1A, 0x00, 0x00]));
    assert_eq!(writes[6], Vec::from([0x0C, 0x97, 0x1C]));
}

#[test]
fn invalid_threshold_is_rejected() {
    let mut gauge = Max17048::new(MockI2c::default());
    assert_eq!(gauge.set_low_soc_threshold(0), Err(Error::InvalidThreshold));
}

#[test]
fn raw_register_escape_hatch() {
    let mut gauge = Max17048::new(MockI2c::new([word(0x0012)]));
    assert_eq!(gauge.read_register(0x08).unwrap(), 0x0012);
    gauge.write_register(0x0A, 0x8030).unwrap();
    assert_eq!(gauge.release().writes()[1], Vec::from([0x0A, 0x80, 0x30]));
}

#[cfg(feature = "async")]
#[test]
fn async_smoke() {
    let mut gauge = Max17048::new(MockI2c::new([word(0xB900)]));
    assert_eq!(block_on(gauge.vcell_millivolts_async()).unwrap(), 3_700);
}
