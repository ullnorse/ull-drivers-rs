extern crate std;

use super::*;
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

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct MockI2cError;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct MockPinError;

impl embedded_hal::digital::Error for MockPinError {
    fn kind(&self) -> embedded_hal::digital::ErrorKind {
        embedded_hal::digital::ErrorKind::Other
    }
}

impl embedded_hal::i2c::Error for MockI2cError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct Write {
    address: u8,
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct MockI2c {
    writes: Vec<Write>,
    fail_after: Option<usize>,
}

impl MockI2c {
    fn new() -> Self {
        Self {
            writes: Vec::new(),
            fail_after: None,
        }
    }

    fn fail_after(write_count: usize) -> Self {
        Self {
            writes: Vec::new(),
            fail_after: Some(write_count),
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
        if self.fail_after == Some(self.writes.len()) {
            return Err(MockI2cError);
        }

        for operation in operations {
            match operation {
                Operation::Read(_) => return Err(MockI2cError),
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
    ) -> core::result::Result<(), Self::Error> {
        if self.fail_after == Some(self.writes.len()) {
            return Err(MockI2cError);
        }

        for operation in operations {
            match operation {
                embedded_hal_async::i2c::Operation::Read(_) => return Err(MockI2cError),
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
    calls_us: Vec<u32>,
}

impl DelayNs for MockDelay {
    fn delay_ns(&mut self, ns: u32) {
        self.calls_us.push(ns.div_ceil(1_000));
    }
}

#[cfg(feature = "async")]
#[derive(Debug, Default)]
struct MockAsyncDelay {
    calls_us: Vec<u32>,
}

#[cfg(feature = "async")]
impl embedded_hal_async::delay::DelayNs for MockAsyncDelay {
    async fn delay_ns(&mut self, ns: u32) {
        self.calls_us.push(ns.div_ceil(1_000));
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

#[derive(Debug, Default)]
struct MockResetPin {
    states: Vec<bool>,
    fail_on_call: Option<usize>,
}

impl MockResetPin {
    fn fail_on_call(call_index: usize) -> Self {
        Self {
            states: Vec::new(),
            fail_on_call: Some(call_index),
        }
    }

    fn record(&mut self, high: bool) -> core::result::Result<(), MockPinError> {
        if self.fail_on_call == Some(self.states.len()) {
            return Err(MockPinError);
        }

        self.states.push(high);
        Ok(())
    }
}

impl embedded_hal::digital::ErrorType for MockResetPin {
    type Error = MockPinError;
}

impl OutputPin for MockResetPin {
    fn set_low(&mut self) -> core::result::Result<(), Self::Error> {
        self.record(false)
    }

    fn set_high(&mut self) -> core::result::Result<(), Self::Error> {
        self.record(true)
    }
}

#[test]
fn custom_address_accepts_only_supported_display_addresses() {
    assert_eq!(Address::custom(0x3C), Some(Address::DEFAULT));
    assert_eq!(Address::custom(0x3D), Some(Address::ALTERNATE));
    assert_eq!(Address::custom(0x00), None);
    assert_eq!(Address::custom(0x3B), None);
    assert_eq!(Address::custom(0x3E), None);
    assert_eq!(Address::custom(0x7F), None);
}

#[test]
fn typed_page_and_line_values_reject_out_of_range_inputs() {
    assert_eq!(Page::<DisplaySize128x64>::new(7).unwrap().as_u8(), 7);
    assert_eq!(Page::<DisplaySize128x64>::new(8), None);
    assert_eq!(Page::<DisplaySize128x32>::new(3).unwrap().as_u8(), 3);
    assert_eq!(Page::<DisplaySize128x32>::new(4), None);
    assert_eq!(
        DisplayLine::<DisplaySize128x64>::new(63).unwrap().as_u8(),
        63
    );
    assert_eq!(DisplayLine::<DisplaySize128x64>::new(64), None);
    assert_eq!(
        DisplayLine::<DisplaySize96x16>::new(15).unwrap().as_u8(),
        15
    );
    assert_eq!(DisplayLine::<DisplaySize96x16>::new(16), None);
    assert_eq!(DisplayOffset::new(63).unwrap().as_u8(), 63);
    assert_eq!(DisplayOffset::new(64), None);
}

#[test]
fn page_range_requires_start_before_end() {
    let start = Page::<DisplaySize128x64>::new(1).unwrap();
    let end = Page::<DisplaySize128x64>::new(3).unwrap();

    assert_eq!(PageRange::new(start, end).unwrap().start(), start);
    assert_eq!(PageRange::new(start, end).unwrap().end(), end);
    assert_eq!(PageRange::new(end, start), None);
}

#[test]
fn vertical_scroll_area_respects_mux_constraints() {
    let whole = VerticalScrollArea::<DisplaySize128x32>::whole_display();
    assert_eq!(whole.top_fixed_rows().as_u8(), 0);
    assert_eq!(whole.scroll_rows().as_u8(), 32);

    let area = VerticalScrollArea::<DisplaySize128x32>::new(
        RowCount::new(8).unwrap(),
        RowCount::new(24).unwrap(),
    )
    .unwrap();
    assert!(area.supports_offset(DisplayLine::new(23).unwrap()));
    assert!(!area.supports_offset(DisplayLine::new(24).unwrap()));
    assert!(
        VerticalScrollArea::<DisplaySize128x32>::new(
            RowCount::new(9).unwrap(),
            RowCount::new(24).unwrap(),
        )
        .is_none()
    );
}

#[test]
fn new_uses_default_i2c_address() {
    let display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate0);
    assert_eq!(display.address(), 0x3C);
}

#[test]
fn init_writes_expected_sequence_for_128x64_internal_vcc() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate0)
        .into_buffered_graphics_mode();

    display.init().unwrap();
    let i2c = display.release();

    assert_eq!(i2c.writes[0].address, 0x3C);
    assert_eq!(
        i2c.writes[0].bytes,
        Vec::from([
            0x00, 0xAE, 0xD5, 0x80, 0xA8, 0x3F, 0xD3, 0x00, 0x40, 0x8D, 0x14, 0x20, 0x00
        ])
    );
    assert_eq!(i2c.writes[1].bytes, Vec::from([0x00, 0xA0, 0xC0]));
    assert_eq!(
        i2c.writes[2].bytes,
        Vec::from([
            0x00, 0xDA, 0x12, 0x81, 0x7F, 0xD9, 0xF1, 0xA3, 0x00, 0x40, 0xDB, 0x40, 0xA4, 0xA6,
            0x2E, 0xAF
        ])
    );
}

#[test]
fn init_uses_datasheet_default_contrast() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize96x16, Rotation::Rotate0);

    display.init().unwrap();
    let i2c = display.release();

    assert_eq!(
        i2c.writes[2].bytes,
        Vec::from([
            0x00, 0xDA, 0x02, 0x81, 0x7F, 0xD9, 0xF1, 0xA3, 0x00, 0x10, 0xDB, 0x40, 0xA4, 0xA6,
            0x2E, 0xAF
        ])
    );
}

#[test]
fn init_with_reset_pulses_reset_pin_before_commands() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate0);
    let mut reset = MockResetPin::default();
    let mut delay = MockDelay::default();

    display.init_with_reset(&mut reset, &mut delay).unwrap();
    let i2c = display.release();

    assert_eq!(reset.states, Vec::from([false, true]));
    assert_eq!(delay.calls_us, Vec::from([3, 3]));
    assert_eq!(i2c.writes[0].bytes[1], 0xAE);
}

#[test]
fn init_with_reset_propagates_pin_errors() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate0);
    let mut reset = MockResetPin::fail_on_call(1);
    let mut delay = MockDelay::default();

    assert_eq!(
        display.init_with_reset(&mut reset, &mut delay),
        Err(InitError::ResetPin(MockPinError))
    );
}

#[test]
fn init_uses_external_vcc_values_and_rotation_180() {
    let mut display = Ssd1306::with_address(
        MockI2c::new(),
        DisplaySize128x32,
        Rotation::Rotate0,
        Address::ALTERNATE,
    );

    display
        .init_with_config(Config {
            orientation: Orientation::from(Rotation::Rotate180),
            power_source: PowerSource::External,
            contrast: 0xCF,
        })
        .unwrap();
    let i2c = display.release();

    assert_eq!(i2c.writes[0].address, 0x3D);
    assert_eq!(i2c.writes[1].bytes, Vec::from([0x00, 0xA1, 0xC8]));
    assert_eq!(
        i2c.writes[2].bytes,
        Vec::from([
            0x00, 0xDA, 0x02, 0x81, 0xCF, 0xD9, 0x22, 0xA3, 0x00, 0x20, 0xDB, 0x40, 0xA4, 0xA6,
            0x2E, 0xAF
        ])
    );
}

#[test]
fn set_pixel_uses_page_layout() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate0)
        .into_buffered_graphics_mode();

    display.set_pixel(0, 0, true);
    display.set_pixel(1, 9, true);
    display.set_pixel(1, 9, false);
    display.set_pixel(127, 63, true);

    assert_eq!(display.buffer()[0], 0x01);
    assert_eq!(display.buffer()[128 + 1], 0x00);
    assert_eq!(display.buffer()[1023], 0x80);
}

#[test]
fn set_pixel_ignores_out_of_bounds_coordinates() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x32, Rotation::Rotate0)
        .into_buffered_graphics_mode();

    display.set_pixel(128, 0, true);
    display.set_pixel(0, 32, true);

    assert!(display.buffer().iter().all(|&byte| byte == 0));
}

#[test]
fn rotation_180_keeps_framebuffer_coordinates_canonical() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate180)
        .into_buffered_graphics_mode();

    display.set_pixel(0, 0, true);

    assert_eq!(display.buffer()[0], 0x01);
}

#[test]
fn set_rotation_preserves_existing_buffered_pixels() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate0)
        .into_buffered_graphics_mode();

    display.set_pixel(0, 0, true);
    display.set_pixel(4, 10, true);
    display.set_rotation(Rotation::Rotate180).unwrap();

    assert_eq!(display.buffer()[0], 0x01);
    assert_eq!(display.buffer()[128 + 4], 0x04);
}

#[test]
fn flush_sets_address_window_then_sends_full_framebuffer() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x32, Rotation::Rotate0)
        .into_buffered_graphics_mode();
    display.buffer_mut()[0] = 0xAA;
    display.buffer_mut()[1] = 0x55;

    display.flush().unwrap();
    let i2c = display.release();

    assert_eq!(i2c.writes[0].bytes, Vec::from([0x00, 0x20, 0x00]));
    assert_eq!(
        i2c.writes[1].bytes,
        Vec::from([0x00, 0x21, 0x00, 0x7F, 0x22, 0x00, 0x03])
    );
    assert_eq!(i2c.writes[2].bytes[..3], [0x40, 0xAA, 0x55]);
    assert_eq!(i2c.writes.len(), 34);
}

#[test]
fn flush_area_only_sends_selected_columns_and_pages() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x32, Rotation::Rotate0)
        .into_buffered_graphics_mode();

    display.buffer_mut()[128 + 1] = 0x12;
    display.buffer_mut()[128 + 2] = 0x34;

    display.flush_area(1, 9, 2, 2).unwrap();
    let i2c = display.release();

    assert_eq!(i2c.writes[0].bytes, Vec::from([0x00, 0x20, 0x00]));
    assert_eq!(
        i2c.writes[1].bytes,
        Vec::from([0x00, 0x21, 0x01, 0x02, 0x22, 0x01, 0x01])
    );
    assert_eq!(i2c.writes[2].bytes, Vec::from([0x40, 0x12, 0x34]));
}

#[test]
fn flush_area_rounds_vertical_range_to_whole_pages() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x32, Rotation::Rotate0)
        .into_buffered_graphics_mode();

    display.buffer_mut()[0] = 0xAA;
    display.buffer_mut()[128] = 0x55;

    display.flush_area(0, 7, 1, 2).unwrap();
    let i2c = display.release();

    assert_eq!(
        i2c.writes[1].bytes,
        Vec::from([0x00, 0x21, 0x00, 0x00, 0x22, 0x00, 0x01])
    );
    assert_eq!(i2c.writes[2].bytes, Vec::from([0x40, 0xAA]));
    assert_eq!(i2c.writes[3].bytes, Vec::from([0x40, 0x55]));
}

#[test]
fn start_line_and_offset_write_expected_commands() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate0);

    display
        .set_display_start_line(DisplayLine::<DisplaySize128x64>::new(12).unwrap())
        .unwrap();
    display
        .set_display_offset(DisplayOffset::new(7).unwrap())
        .unwrap();

    let i2c = display.release();

    assert_eq!(i2c.writes[0].bytes, Vec::from([0x00, 0x4C]));
    assert_eq!(i2c.writes[1].bytes, Vec::from([0x00, 0xD3, 0x07]));
}

#[test]
fn display_offset_uses_full_controller_range() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x32, Rotation::Rotate0);

    display
        .set_display_offset(DisplayOffset::new(40).unwrap())
        .unwrap();

    let i2c = display.release();
    assert_eq!(i2c.writes[0].bytes, Vec::from([0x00, 0xD3, 0x28]));
}

#[test]
fn configure_horizontal_scroll_stops_scroll_then_programs_sequence() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate0);

    display
        .configure_horizontal_scroll(
            ScrollDirection::Left,
            PageRange::new(
                Page::<DisplaySize128x64>::new(1).unwrap(),
                Page::<DisplaySize128x64>::new(3).unwrap(),
            )
            .unwrap(),
            ScrollFrameInterval::Frames25,
        )
        .unwrap();

    let i2c = display.release();

    assert_eq!(i2c.writes[0].bytes, Vec::from([0x00, 0x2E]));
    assert_eq!(
        i2c.writes[1].bytes,
        Vec::from([0x00, 0x27, 0x00, 0x01, 0x06, 0x03, 0x00, 0xFF])
    );
}

#[test]
fn configure_diagonal_scroll_stops_scroll_then_programs_sequence() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate0);

    display
        .configure_diagonal_scroll(
            ScrollDirection::Right,
            PageRange::new(
                Page::<DisplaySize128x64>::new(0).unwrap(),
                Page::<DisplaySize128x64>::new(7).unwrap(),
            )
            .unwrap(),
            ScrollFrameInterval::Frames2,
            VerticalScrollArea::<DisplaySize128x64>::whole_display(),
            DisplayLine::<DisplaySize128x64>::new(1).unwrap(),
        )
        .unwrap();

    let i2c = display.release();

    assert_eq!(i2c.writes[0].bytes, Vec::from([0x00, 0x2E]));
    assert_eq!(i2c.writes[1].bytes, Vec::from([0x00, 0xA3, 0x00, 0x40]));
    assert_eq!(
        i2c.writes[2].bytes,
        Vec::from([0x00, 0x29, 0x00, 0x00, 0x07, 0x07, 0x01])
    );
}

#[test]
fn configure_diagonal_scroll_rejects_offset_outside_scroll_area() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate0);

    let error = display
        .configure_diagonal_scroll(
            ScrollDirection::Right,
            PageRange::whole_display(),
            ScrollFrameInterval::Frames2,
            VerticalScrollArea::new(RowCount::new(8).unwrap(), RowCount::new(8).unwrap()).unwrap(),
            DisplayLine::new(8).unwrap(),
        )
        .unwrap_err();

    assert_eq!(
        error,
        Error::InvalidArgument(InvalidArgument::VerticalScrollOffsetOutOfRange)
    );
    assert!(display.release().writes.is_empty());
}

#[test]
fn start_and_stop_scroll_emit_single_commands() {
    let display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate0);

    let display = display.start_scroll().unwrap();
    let display = display.stop_scroll().unwrap().finish_scroll_rewrite();

    let i2c = display.release();

    assert_eq!(i2c.writes[0].bytes, Vec::from([0x00, 0x2F]));
    assert_eq!(i2c.writes[1].bytes, Vec::from([0x00, 0x2E]));
}

#[test]
fn restore_display_rewrites_framebuffer_after_stopping_scroll() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x32, Rotation::Rotate0)
        .into_buffered_graphics_mode();
    display.buffer_mut()[0] = 0xAA;
    display.buffer_mut()[1] = 0x55;

    let display = display.start_scroll().unwrap();
    let display = display.stop_scroll().unwrap();
    let display = display.restore_display().unwrap();

    let i2c = display.release();

    assert_eq!(i2c.writes[0].bytes, Vec::from([0x00, 0x2F]));
    assert_eq!(i2c.writes[1].bytes, Vec::from([0x00, 0x2E]));
    assert_eq!(i2c.writes[2].bytes, Vec::from([0x00, 0x20, 0x00]));
    assert_eq!(
        i2c.writes[3].bytes,
        Vec::from([0x00, 0x21, 0x00, 0x7F, 0x22, 0x00, 0x03])
    );
    assert_eq!(i2c.writes[4].bytes[..3], [0x40, 0xAA, 0x55]);
}

#[test]
fn start_scroll_failure_preserves_driver() {
    let display = Ssd1306::new(MockI2c::fail_after(0), DisplaySize128x64, Rotation::Rotate0);

    let error = display.start_scroll().unwrap_err();
    let (display, error) = error.into_parts();

    assert_eq!(error, Error::Bus(MockI2cError));
    assert!(display.release().writes.is_empty());
}

#[test]
fn stop_scroll_failure_preserves_driver() {
    let display = Ssd1306::new(MockI2c::fail_after(1), DisplaySize128x64, Rotation::Rotate0);
    let display = display.start_scroll().unwrap();

    let error = display.stop_scroll().unwrap_err();
    let (display, error) = error.into_parts();

    assert_eq!(error, Error::Bus(MockI2cError));

    let i2c = display.release();
    assert_eq!(i2c.writes.len(), 1);
    assert_eq!(i2c.writes[0].bytes, Vec::from([0x00, 0x2F]));
}

#[test]
fn set_orientation_supports_all_segment_and_com_combinations() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate0)
        .into_buffered_graphics_mode();

    display
        .set_orientation(Orientation {
            segment_remap: SegmentRemap::Remapped,
            com_scan_direction: ComScanDirection::Normal,
        })
        .unwrap();
    display
        .set_orientation(Orientation {
            segment_remap: SegmentRemap::Normal,
            com_scan_direction: ComScanDirection::Remapped,
        })
        .unwrap();

    let i2c = display.release();
    assert_eq!(i2c.writes[0].bytes, Vec::from([0x00, 0xA1, 0xC0]));
    assert_eq!(i2c.writes[1].bytes, Vec::from([0x00, 0xA0, 0xC8]));
}

#[test]
fn set_orientation_preserves_existing_buffered_pixels() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate0)
        .into_buffered_graphics_mode();

    display.set_pixel(0, 0, true);
    display
        .set_orientation(Orientation {
            segment_remap: SegmentRemap::Remapped,
            com_scan_direction: ComScanDirection::Normal,
        })
        .unwrap();

    assert_eq!(display.buffer()[0], 0x01);
}

#[test]
fn bus_errors_are_propagated() {
    let mut display = Ssd1306::new(MockI2c::fail_after(0), DisplaySize128x64, Rotation::Rotate0);

    assert_eq!(
        display.set_rotation(Rotation::Rotate0),
        Err(Error::Bus(MockI2cError))
    );
}

#[test]
fn set_orientation_failure_preserves_previous_orientation() {
    let mut display = Ssd1306::new(MockI2c::fail_after(0), DisplaySize128x64, Rotation::Rotate0);

    assert_eq!(display.orientation(), Orientation::ROTATE_0);
    assert_eq!(
        display.set_orientation(Orientation::ROTATE_180),
        Err(Error::Bus(MockI2cError))
    );
    assert_eq!(display.orientation(), Orientation::ROTATE_0);
    assert_eq!(display.rotation(), Some(Rotation::Rotate0));
}

#[test]
fn init_with_config_failure_preserves_previous_config() {
    let mut display = Ssd1306::new(MockI2c::fail_after(2), DisplaySize128x64, Rotation::Rotate0);

    let initial_orientation = display.orientation();
    assert_eq!(initial_orientation, Orientation::ROTATE_0);
    assert_eq!(display.rotation(), Some(Rotation::Rotate0));

    assert_eq!(
        display.init_with_config(Config {
            orientation: Orientation::ROTATE_180,
            power_source: PowerSource::External,
            contrast: 0xCF,
        }),
        Err(Error::Bus(MockI2cError))
    );

    assert_eq!(display.orientation(), initial_orientation);
    assert_eq!(display.rotation(), Some(Rotation::Rotate0));

    let i2c = display.release();
    assert_eq!(i2c.writes.len(), 2);
}

#[test]
fn clear_zeros_buffer() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize96x16, Rotation::Rotate0)
        .into_buffered_graphics_mode();
    display.buffer_mut().fill(0xFF);

    display.clear();

    assert!(display.buffer().iter().all(|&byte| byte == 0));
}

#[test]
fn raw_mode_exposes_direct_command_and_data_writes() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate0);

    display.write_command(0xAE).unwrap();
    display.write_commands(&[0xA6, 0xAF]).unwrap();
    display.write_data(&[0x12, 0x34]).unwrap();

    let i2c = display.release();

    assert_eq!(i2c.writes[0].bytes, Vec::from([0x00, 0xAE]));
    assert_eq!(i2c.writes[1].bytes, Vec::from([0x00, 0xA6, 0xAF]));
    assert_eq!(i2c.writes[2].bytes, Vec::from([0x40, 0x12, 0x34]));
}

#[cfg(feature = "async")]
#[test]
fn async_raw_mode_exposes_direct_command_and_data_writes() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate0);

    block_on(async {
        display.write_command_async(0xAE).await.unwrap();
        display.write_commands_async(&[0xA6, 0xAF]).await.unwrap();
        display.write_data_async(&[0x12, 0x34]).await.unwrap();
    });

    let i2c = display.release();

    assert_eq!(i2c.writes[0].bytes, Vec::from([0x00, 0xAE]));
    assert_eq!(i2c.writes[1].bytes, Vec::from([0x00, 0xA6, 0xAF]));
    assert_eq!(i2c.writes[2].bytes, Vec::from([0x40, 0x12, 0x34]));
}

#[cfg(feature = "async")]
#[test]
fn async_flush_sets_addressing_then_sends_framebuffer() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x32, Rotation::Rotate0)
        .into_buffered_graphics_mode();
    display.buffer_mut()[0] = 0xAA;
    display.buffer_mut()[1] = 0x55;

    block_on(display.flush_async()).unwrap();
    let i2c = display.release();

    assert_eq!(i2c.writes[0].bytes, Vec::from([0x00, 0x20, 0x00]));
    assert_eq!(
        i2c.writes[1].bytes,
        Vec::from([0x00, 0x21, 0x00, 0x7F, 0x22, 0x00, 0x03])
    );
    assert_eq!(i2c.writes[2].bytes[..3], [0x40, 0xAA, 0x55]);
}

#[cfg(feature = "async")]
#[test]
fn async_restore_display_rewrites_framebuffer_after_stopping_scroll() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x32, Rotation::Rotate0)
        .into_buffered_graphics_mode();
    display.buffer_mut()[0] = 0xAA;
    display.buffer_mut()[1] = 0x55;

    let display = block_on(display.start_scroll_async()).unwrap();
    let display = block_on(display.stop_scroll_async()).unwrap();
    let display = block_on(display.restore_display_async()).unwrap();

    let i2c = display.release();

    assert_eq!(i2c.writes[0].bytes, Vec::from([0x00, 0x2F]));
    assert_eq!(i2c.writes[1].bytes, Vec::from([0x00, 0x2E]));
    assert_eq!(i2c.writes[2].bytes, Vec::from([0x00, 0x20, 0x00]));
    assert_eq!(
        i2c.writes[3].bytes,
        Vec::from([0x00, 0x21, 0x00, 0x7F, 0x22, 0x00, 0x03])
    );
    assert_eq!(i2c.writes[4].bytes[..3], [0x40, 0xAA, 0x55]);
}

#[cfg(feature = "async")]
#[test]
fn init_with_reset_async_pulses_reset_pin_before_commands() {
    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate0);
    let mut reset = MockResetPin::default();
    let mut delay = MockAsyncDelay::default();

    block_on(display.init_with_reset_async(&mut reset, &mut delay)).unwrap();
    let i2c = display.release();

    assert_eq!(reset.states, Vec::from([false, true]));
    assert_eq!(delay.calls_us, Vec::from([3, 3]));
    assert_eq!(i2c.writes[0].bytes[1], 0xAE);
}

#[cfg(feature = "async")]
#[test]
fn set_orientation_async_failure_preserves_previous_orientation() {
    let mut display = Ssd1306::new(MockI2c::fail_after(0), DisplaySize128x64, Rotation::Rotate0);

    assert_eq!(display.orientation(), Orientation::ROTATE_0);

    let result = block_on(display.set_orientation_async(Orientation::ROTATE_180));

    assert_eq!(result, Err(Error::Bus(MockI2cError)));
    assert_eq!(display.orientation(), Orientation::ROTATE_0);
    assert_eq!(display.rotation(), Some(Rotation::Rotate0));
}

#[cfg(feature = "async")]
#[test]
fn init_with_config_async_failure_preserves_previous_config() {
    let mut display = Ssd1306::new(MockI2c::fail_after(2), DisplaySize128x64, Rotation::Rotate0);

    let initial_orientation = display.orientation();
    assert_eq!(initial_orientation, Orientation::ROTATE_0);
    assert_eq!(display.rotation(), Some(Rotation::Rotate0));

    let result = block_on(display.init_with_config_async(Config {
        orientation: Orientation::ROTATE_180,
        power_source: PowerSource::External,
        contrast: 0xCF,
    }));

    assert_eq!(result, Err(Error::Bus(MockI2cError)));
    assert_eq!(display.orientation(), initial_orientation);
    assert_eq!(display.rotation(), Some(Rotation::Rotate0));

    let i2c = display.release();
    assert_eq!(i2c.writes.len(), 2);
}

fn assert_send<T: Send>() {}

#[test]
fn driver_is_send_when_bus_is_send() {
    assert_send::<Ssd1306<MockI2c, DisplaySize128x64, RawMode>>();
}

#[cfg(feature = "graphics")]
#[test]
fn draw_target_updates_framebuffer_without_flushing() {
    use embedded_graphics_core::{
        Pixel, draw_target::DrawTarget, pixelcolor::BinaryColor, prelude::Point,
    };

    let mut display = Ssd1306::new(MockI2c::new(), DisplaySize128x64, Rotation::Rotate0)
        .into_buffered_graphics_mode();

    display
        .draw_iter([Pixel(Point::new(2, 3), BinaryColor::On)])
        .unwrap();

    assert_eq!(display.buffer()[2], 0x08);
    let i2c = display.release();
    assert!(i2c.writes.is_empty());
}
