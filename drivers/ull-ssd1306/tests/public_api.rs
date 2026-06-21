#[cfg(feature = "async")]
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::i2c::{ErrorKind, ErrorType, I2c, Operation, SevenBitAddress};
#[cfg(feature = "async")]
use std::pin::pin;
use std::vec::Vec;
use ull_ssd1306::{
    Address, DisplaySize128x32, PageRange, Rotation, ScrollDirection, ScrollFrameInterval, Ssd1306,
};

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct MockI2cError;

impl embedded_hal::i2c::Error for MockI2cError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct WriteFrame {
    address: u8,
    bytes: Vec<u8>,
}

#[derive(Debug, Default)]
struct MockI2c {
    writes: Vec<WriteFrame>,
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
                Operation::Read(_) => return Err(MockI2cError),
                Operation::Write(bytes) => self.writes.push(WriteFrame {
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
    ) -> core::result::Result<(), Self::Error> {
        for operation in operations {
            match operation {
                embedded_hal_async::i2c::Operation::Read(_) => return Err(MockI2cError),
                embedded_hal_async::i2c::Operation::Write(bytes) => self.writes.push(WriteFrame {
                    address,
                    bytes: bytes.to_vec(),
                }),
            }
        }

        Ok(())
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct MockPinError;

impl embedded_hal::digital::Error for MockPinError {
    fn kind(&self) -> embedded_hal::digital::ErrorKind {
        embedded_hal::digital::ErrorKind::Other
    }
}

#[derive(Debug, Default)]
struct MockDelay {
    calls_us: Vec<u32>,
}

impl DelayNs for MockDelay {
    fn delay_ns(&mut self, ns: u32) {
        self.calls_us.push(ns.div_ceil(1_000));
    }
}

#[derive(Debug, Default)]
struct MockResetPin {
    levels: Vec<bool>,
}

impl embedded_hal::digital::ErrorType for MockResetPin {
    type Error = MockPinError;
}

impl OutputPin for MockResetPin {
    fn set_low(&mut self) -> core::result::Result<(), Self::Error> {
        self.levels.push(false);
        Ok(())
    }

    fn set_high(&mut self) -> core::result::Result<(), Self::Error> {
        self.levels.push(true);
        Ok(())
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

#[test]
fn blocking_buffered_flow_supports_init_draw_and_partial_flush() {
    let mut display = Ssd1306::with_address(
        MockI2c::default(),
        DisplaySize128x32,
        Rotation::Rotate180,
        Address::ALTERNATE,
    )
    .into_buffered_graphics_mode();
    let mut delay = MockDelay::default();
    let mut reset = MockResetPin::default();

    assert_eq!(display.width(), 128);
    assert_eq!(display.height(), 32);
    assert_eq!(display.address(), 0x3D);
    assert_eq!(display.rotation(), Some(Rotation::Rotate180));

    display.init_with_reset(&mut reset, &mut delay).unwrap();
    display.clear();
    display.set_pixel(1, 9, true);
    display.set_pixel(2, 9, true);
    display.flush_area(1, 9, 2, 1).unwrap();

    let i2c = display.release();

    assert_eq!(reset.levels, Vec::from([false, true]));
    assert_eq!(delay.calls_us, Vec::from([3, 3]));
    assert_eq!(i2c.writes[0].address, 0x3D);
    assert_eq!(i2c.writes[1].bytes, Vec::from([0x00, 0xA1, 0xC8]));
    assert_eq!(i2c.writes[3].bytes, Vec::from([0x00, 0x20, 0x00]));
    assert_eq!(
        i2c.writes[4].bytes,
        Vec::from([0x00, 0x21, 0x01, 0x02, 0x22, 0x01, 0x01])
    );
    assert_eq!(i2c.writes[5].bytes, Vec::from([0x40, 0x02, 0x02]));
}

#[test]
fn scroll_restore_flow_rewrites_buffer_from_outside_the_crate() {
    let mut display = Ssd1306::new(MockI2c::default(), DisplaySize128x32, Rotation::Rotate0)
        .into_buffered_graphics_mode();

    display.set_pixel(0, 0, true);
    display
        .configure_horizontal_scroll(
            ScrollDirection::Left,
            PageRange::<DisplaySize128x32>::whole_display(),
            ScrollFrameInterval::Frames25,
        )
        .unwrap();

    let display = display.start_scroll().unwrap();
    let display = display.stop_scroll().unwrap();
    let display = display.restore_display().unwrap();

    assert_eq!(display.buffer()[0], 0x01);

    let i2c = display.release();

    assert_eq!(i2c.writes[0].bytes, Vec::from([0x00, 0x2E]));
    assert_eq!(
        i2c.writes[1].bytes,
        Vec::from([0x00, 0x27, 0x00, 0x00, 0x06, 0x03, 0x00, 0xFF])
    );
    assert_eq!(i2c.writes[2].bytes, Vec::from([0x00, 0x2F]));
    assert_eq!(i2c.writes[3].bytes, Vec::from([0x00, 0x2E]));
    assert_eq!(i2c.writes[4].bytes, Vec::from([0x00, 0x20, 0x00]));
    assert_eq!(
        i2c.writes[5].bytes,
        Vec::from([0x00, 0x21, 0x00, 0x7F, 0x22, 0x00, 0x03])
    );
    assert_eq!(i2c.writes[6].bytes[0], 0x40);
    assert_eq!(i2c.writes[6].bytes[1], 0x01);
}

#[cfg(feature = "async")]
#[test]
fn async_buffered_mode_smoke_test() {
    let mut display = Ssd1306::new(MockI2c::default(), DisplaySize128x32, Rotation::Rotate0)
        .into_buffered_graphics_mode();

    block_on(async {
        display.init_async().await.unwrap();
        display.set_pixel(3, 4, true);
        display.flush_async().await.unwrap();
    });

    let i2c = display.release();

    assert_eq!(i2c.writes[0].bytes[1], 0xAE);
    assert_eq!(i2c.writes[3].bytes, Vec::from([0x00, 0x20, 0x00]));
    assert_eq!(
        i2c.writes[4].bytes,
        Vec::from([0x00, 0x21, 0x00, 0x7F, 0x22, 0x00, 0x03])
    );
    assert_eq!(i2c.writes[5].bytes[0], 0x40);
    assert_eq!(i2c.writes[5].bytes[4], 0x10);
}
