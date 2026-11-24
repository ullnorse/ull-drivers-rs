use std::cell::RefCell;
use std::rc::Rc;
use std::vec::Vec;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{ErrorType as PinErrorType, InputPin, OutputPin};
use embedded_hal::spi::{ErrorType, Operation, SpiDevice};
use ull_ssd1681::{Config, DeepSleepMode, Error, FRAME_LEN, Ssd1681};

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct MockError;

impl embedded_hal::spi::Error for MockError {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        embedded_hal::spi::ErrorKind::Other
    }
}

impl embedded_hal::digital::Error for MockError {
    fn kind(&self) -> embedded_hal::digital::ErrorKind {
        embedded_hal::digital::ErrorKind::Other
    }
}

#[derive(Debug, Default)]
struct MockSpi {
    writes: Rc<RefCell<Vec<Vec<u8>>>>,
}

impl ErrorType for MockSpi {
    type Error = MockError;
}

impl SpiDevice for MockSpi {
    fn transaction(
        &mut self,
        operations: &mut [Operation<'_, u8>],
    ) -> core::result::Result<(), MockError> {
        for operation in operations {
            if let Operation::Write(bytes) = operation {
                self.writes.borrow_mut().push(bytes.to_vec());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
struct MockOutput;

impl PinErrorType for MockOutput {
    type Error = MockError;
}

impl OutputPin for MockOutput {
    fn set_low(&mut self) -> core::result::Result<(), MockError> {
        Ok(())
    }

    fn set_high(&mut self) -> core::result::Result<(), MockError> {
        Ok(())
    }
}

#[derive(Debug)]
struct MockBusy(bool);

impl PinErrorType for MockBusy {
    type Error = MockError;
}

impl InputPin for MockBusy {
    fn is_high(&mut self) -> core::result::Result<bool, MockError> {
        Ok(self.0)
    }

    fn is_low(&mut self) -> core::result::Result<bool, MockError> {
        Ok(!self.0)
    }
}

#[derive(Debug, Default)]
struct MockDelay;

impl DelayNs for MockDelay {
    fn delay_ns(&mut self, _ns: u32) {}
}

type TestDriver = Ssd1681<MockSpi, MockOutput, MockOutput, MockBusy, MockDelay>;
type Writes = Rc<RefCell<Vec<Vec<u8>>>>;

fn driver(busy: bool) -> (TestDriver, Writes) {
    let spi = MockSpi::default();
    let writes = spi.writes.clone();
    let epd = Ssd1681::new(spi, MockOutput, MockOutput, MockBusy(busy), MockDelay);
    (epd, writes)
}

#[test]
fn blocking_flow_and_release() {
    let (mut epd, writes) = driver(false);
    assert_eq!(epd.config(), Config::default());

    epd.init().unwrap();
    let frame = [0xFF; FRAME_LEN];
    epd.display_full(&frame).unwrap();
    epd.display_partial(&frame).unwrap();
    epd.deep_sleep(DeepSleepMode::Mode1).unwrap();

    let writes = writes.borrow();
    assert_eq!(writes.first(), Some(&Vec::from([0x12])));
    assert_eq!(writes.last(), Some(&Vec::from([0x01])));
    assert_eq!(writes[writes.len() - 2], Vec::from([0x10]));
    assert!(writes.iter().any(|write| write.len() == FRAME_LEN));
    drop(writes);

    let (_spi, _dc, _rst, busy, _delay) = epd.release();
    assert!(!busy.0);
}

#[test]
fn raw_command_escape_hatch() {
    let (mut epd, writes) = driver(false);
    epd.write_command(0x3C, &[0x05]).unwrap();
    assert_eq!(*writes.borrow(), [Vec::from([0x3C]), Vec::from([0x05])]);
}

#[test]
fn stuck_busy_times_out_instead_of_hanging() {
    let (mut epd, _writes) = driver(true);
    assert_eq!(epd.wait_until_idle(2), Err(Error::BusyTimeout));
}

#[cfg(feature = "async")]
mod async_smoke {
    use super::*;
    use core::future::Future;
    use core::pin::pin;
    use core::task::{Context, Poll, Waker};

    impl embedded_hal_async::spi::SpiDevice for MockSpi {
        async fn transaction(
            &mut self,
            operations: &mut [embedded_hal_async::spi::Operation<'_, u8>],
        ) -> core::result::Result<(), MockError> {
            for operation in operations {
                if let embedded_hal_async::spi::Operation::Write(bytes) = operation {
                    self.writes.borrow_mut().push(bytes.to_vec());
                }
            }
            Ok(())
        }
    }

    impl embedded_hal_async::delay::DelayNs for MockDelay {
        async fn delay_ns(&mut self, _ns: u32) {}
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let mut context = Context::from_waker(Waker::noop());
        let mut future = pin!(future);
        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return output;
            }
        }
    }

    #[test]
    fn async_flow() {
        let (mut epd, writes) = driver(false);
        let frame = [0xFF; FRAME_LEN];
        block_on(async {
            epd.init_async().await.unwrap();
            epd.display_partial_async(&frame).await.unwrap();
            epd.deep_sleep_async(DeepSleepMode::Mode2).await.unwrap();
        });
        assert_eq!(writes.borrow().last(), Some(&Vec::from([0x03])));
    }
}
