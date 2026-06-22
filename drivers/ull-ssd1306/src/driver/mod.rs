mod blocking;

#[cfg(feature = "async")]
mod asynchronous;

use core::marker::PhantomData;

#[cfg(feature = "graphics")]
use core::convert::Infallible;

use crate::size::{DisplaySize, private};
use crate::types::{
    Address, ComScanDirection, Config, DisplayLine, Error, InitError, InvalidArgument, Orientation,
    PageRange, PowerSource, Rotation, ScrollDirection, ScrollFrameInterval, SegmentRemap,
    VerticalScrollArea,
};

const CONTROL_COMMAND: u8 = 0x00;
const CONTROL_DATA: u8 = 0x40;
const CHUNK_SIZE: usize = 16;

/// Error returned when a scroll state transition fails.
#[derive(Debug)]
pub struct StateChangeError<DRIVER, BusError> {
    driver: DRIVER,
    error: Error<BusError>,
}

impl<DRIVER, BusError> StateChangeError<DRIVER, BusError> {
    fn new(driver: DRIVER, error: Error<BusError>) -> Self {
        Self { driver, error }
    }

    /// Returns the preserved driver instance.
    #[must_use]
    pub fn driver(&self) -> &DRIVER {
        &self.driver
    }

    /// Returns the underlying driver error.
    #[must_use]
    pub fn error(&self) -> &Error<BusError> {
        &self.error
    }

    /// Splits the error into the preserved driver and underlying error.
    #[must_use]
    pub fn into_parts(self) -> (DRIVER, Error<BusError>) {
        (self.driver, self.error)
    }
}

impl<DRIVER, BusError> core::fmt::Display for StateChangeError<DRIVER, BusError>
where
    BusError: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "scroll state change failed: {}", self.error)
    }
}

impl<DRIVER, BusError> core::error::Error for StateChangeError<DRIVER, BusError>
where
    DRIVER: core::fmt::Debug,
    BusError: core::fmt::Debug,
    Error<BusError>: core::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        Some(&self.error)
    }
}

type StateChangeResult<NEXT, CURRENT, BusError> =
    core::result::Result<NEXT, StateChangeError<CURRENT, BusError>>;

/// Marker type for the raw command mode.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct RawMode;

/// Marker type for inactive hardware scrolling.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct ScrollInactive;

/// Marker type for active hardware scrolling.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct ScrollActive;

/// Marker type for a stopped scroll that still requires GDDRAM to be rewritten.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct ScrollRestoreRequired;

/// Marker type for the buffered graphics mode.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BufferedGraphicsMode<SIZE>
where
    SIZE: DisplaySize,
{
    buffer: SIZE::Buffer,
}

impl<SIZE> Default for BufferedGraphicsMode<SIZE>
where
    SIZE: DisplaySize,
{
    fn default() -> Self {
        Self {
            buffer: <SIZE as private::DisplaySizePrivate>::make_buffer(),
        }
    }
}

/// SSD1306 controller in a selected transport, panel size, mode, and scroll state.
#[derive(Debug)]
pub struct Ssd1306<DI, SIZE, MODE, SCROLL = ScrollInactive> {
    i2c: DI,
    address: u8,
    mode: MODE,
    config: Config,
    _size: PhantomData<SIZE>,
    _scroll: PhantomData<SCROLL>,
}

type DriverStateChangeResult<DI, SIZE, MODE, CurrentScroll, NextScroll, BusError> =
    StateChangeResult<
        Ssd1306<DI, SIZE, MODE, NextScroll>,
        Ssd1306<DI, SIZE, MODE, CurrentScroll>,
        BusError,
    >;

impl<DI, SIZE> Ssd1306<DI, SIZE, RawMode, ScrollInactive>
where
    SIZE: DisplaySize,
{
    /// Creates a new SSD1306 driver in raw command mode using the default `0x3C` address.
    #[must_use]
    pub fn new(i2c: DI, size: SIZE, rotation: Rotation) -> Self {
        Self::with_address(i2c, size, rotation, Address::DEFAULT)
    }

    /// Creates a new SSD1306 driver in raw command mode using the selected address.
    #[must_use]
    pub fn with_address(i2c: DI, size: SIZE, rotation: Rotation, address: Address) -> Self {
        Self::with_address_and_orientation(i2c, size, rotation.into(), address)
    }

    /// Creates a new SSD1306 driver in raw command mode with a fully specified orientation.
    #[must_use]
    pub fn new_with_orientation(i2c: DI, size: SIZE, orientation: Orientation) -> Self {
        Self::with_address_and_orientation(i2c, size, orientation, Address::DEFAULT)
    }

    /// Creates a new SSD1306 driver in raw command mode with a selected address and orientation.
    #[must_use]
    pub fn with_address_and_orientation(
        i2c: DI,
        size: SIZE,
        orientation: Orientation,
        address: Address,
    ) -> Self {
        let _ = size;
        Self {
            i2c,
            address: address.as_u8(),
            mode: RawMode,
            config: Config {
                orientation,
                ..Config::default()
            },
            _size: PhantomData,
            _scroll: PhantomData,
        }
    }

    /// Attaches a framebuffer and enters buffered graphics mode.
    #[must_use]
    pub fn into_buffered_graphics_mode(
        self,
    ) -> Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>, ScrollInactive> {
        Ssd1306 {
            i2c: self.i2c,
            address: self.address,
            mode: BufferedGraphicsMode::<SIZE>::default(),
            config: self.config,
            _size: PhantomData,
            _scroll: PhantomData,
        }
    }
}

impl<DI, SIZE, SCROLL> Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>, SCROLL>
where
    SIZE: DisplaySize,
{
    /// Drops the framebuffer and enters raw command mode.
    #[must_use]
    pub fn into_raw_mode(self) -> Ssd1306<DI, SIZE, RawMode, SCROLL> {
        Ssd1306 {
            i2c: self.i2c,
            address: self.address,
            mode: RawMode,
            config: self.config,
            _size: PhantomData,
            _scroll: PhantomData,
        }
    }

    /// Clears the framebuffer.
    pub fn clear(&mut self) {
        self.mode.buffer.as_mut().fill(0);
    }

    /// Sets one pixel in the local framebuffer.
    pub fn set_pixel(&mut self, x: u32, y: u32, on: bool) {
        let width = u32::from(SIZE::WIDTH);
        let height = u32::from(SIZE::HEIGHT);

        if x >= width || y >= height {
            return;
        }

        let index = x as usize + ((y as usize / 8) * SIZE::WIDTH as usize);
        let bit = 1u8 << (y as u8 & 7);
        let buffer = self.mode.buffer.as_mut();

        if on {
            buffer[index] |= bit;
        } else {
            buffer[index] &= !bit;
        }
    }

    /// Returns the framebuffer as bytes in SSD1306 page order.
    #[must_use]
    pub fn buffer(&self) -> &[u8] {
        self.mode.buffer.as_ref()
    }

    /// Returns the mutable framebuffer bytes in SSD1306 page order.
    #[must_use]
    pub fn buffer_mut(&mut self) -> &mut [u8] {
        self.mode.buffer.as_mut()
    }
}

impl<DI, SIZE, MODE, SCROLL> Ssd1306<DI, SIZE, MODE, SCROLL> {
    /// Returns the configured orientation.
    #[must_use]
    pub const fn orientation(&self) -> Orientation {
        self.config.orientation
    }

    /// Returns the matching convenience rotation, if the current orientation matches one.
    #[must_use]
    pub const fn rotation(&self) -> Option<Rotation> {
        self.config.orientation.rotation()
    }

    /// Returns the configured 7-bit I2C address.
    #[must_use]
    pub const fn address(&self) -> u8 {
        self.address
    }

    /// Releases the underlying I2C peripheral.
    #[must_use]
    pub fn release(self) -> DI {
        self.i2c
    }

    fn into_scroll_state<NextScroll>(self) -> Ssd1306<DI, SIZE, MODE, NextScroll> {
        Ssd1306 {
            i2c: self.i2c,
            address: self.address,
            mode: self.mode,
            config: self.config,
            _size: PhantomData,
            _scroll: PhantomData,
        }
    }
}

impl<DI, SIZE, MODE, SCROLL> Ssd1306<DI, SIZE, MODE, SCROLL>
where
    SIZE: DisplaySize,
{
    /// Returns the display width in pixels.
    #[must_use]
    pub const fn width(&self) -> u8 {
        SIZE::WIDTH
    }

    /// Returns the display height in pixels.
    #[must_use]
    pub const fn height(&self) -> u8 {
        SIZE::HEIGHT
    }
}

impl<DI, SIZE, MODE> Ssd1306<DI, SIZE, MODE, ScrollRestoreRequired> {
    /// Marks scroll restoration complete after application code has rewritten GDDRAM.
    #[must_use]
    pub fn finish_scroll_rewrite(self) -> Ssd1306<DI, SIZE, MODE, ScrollInactive> {
        self.into_scroll_state()
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct DrawArea {
    start_column: u8,
    end_column: u8,
    start_page: u8,
    end_page: u8,
}

impl DrawArea {
    const fn full<SIZE>() -> Self
    where
        SIZE: DisplaySize,
    {
        Self {
            start_column: 0,
            end_column: SIZE::WIDTH - 1,
            start_page: 0,
            end_page: (SIZE::HEIGHT / 8) - 1,
        }
    }

    fn clipped<SIZE>(x: u32, y: u32, width: u32, height: u32) -> Option<Self>
    where
        SIZE: DisplaySize,
    {
        let max_width = u32::from(SIZE::WIDTH);
        let max_height = u32::from(SIZE::HEIGHT);
        let end_x = x.saturating_add(width).min(max_width);
        let end_y = y.saturating_add(height).min(max_height);

        if x >= end_x || y >= end_y {
            return None;
        }

        Some(Self {
            start_column: x as u8,
            end_column: (end_x - 1) as u8,
            start_page: (y / 8) as u8,
            end_page: ((end_y - 1) / 8) as u8,
        })
    }

    const fn commands(self) -> [u8; 6] {
        [
            0x21,
            self.start_column,
            self.end_column,
            0x22,
            self.start_page,
            self.end_page,
        ]
    }

    fn column_range(self) -> (usize, usize) {
        (
            usize::from(self.start_column),
            usize::from(self.end_column) + 1,
        )
    }

    fn is_full<SIZE>(self) -> bool
    where
        SIZE: DisplaySize,
    {
        self == Self::full::<SIZE>()
    }
}

fn frame_packet<'a>(packet: &'a mut [u8; CHUNK_SIZE + 1], control: u8, chunk: &[u8]) -> &'a [u8] {
    packet[0] = control;
    let len = chunk.len();
    packet[1..1 + len].copy_from_slice(chunk);
    &packet[..1 + len]
}

fn init_prefix<SIZE>(config: Config) -> [u8; 12]
where
    SIZE: DisplaySize,
{
    [
        0xAE,
        0xD5,
        0x80,
        0xA8,
        <SIZE as private::DisplaySizePrivate>::multiplex(),
        0xD3,
        0x00,
        0x40,
        0x8D,
        charge_pump_value(config.power_source),
        0x20,
        0x00,
    ]
}

fn init_suffix<SIZE>(config: Config) -> [u8; 15]
where
    SIZE: DisplaySize,
{
    [
        0xDA,
        <SIZE as private::DisplaySizePrivate>::com_pins(),
        0x81,
        config.contrast,
        0xD9,
        precharge_value(config.power_source),
        0xA3,
        0x00,
        SIZE::HEIGHT,
        0xDB,
        0x40,
        0xA4,
        0xA6,
        0x2E,
        0xAF,
    ]
}

const fn charge_pump_value(power_source: PowerSource) -> u8 {
    match power_source {
        PowerSource::Internal => 0x14,
        PowerSource::External => 0x10,
    }
}

const fn precharge_value(power_source: PowerSource) -> u8 {
    match power_source {
        PowerSource::Internal => 0xF1,
        PowerSource::External => 0x22,
    }
}

const fn scroll_command(direction: ScrollDirection) -> u8 {
    match direction {
        ScrollDirection::Right => 0x26,
        ScrollDirection::Left => 0x27,
    }
}

const fn diagonal_scroll_command(direction: ScrollDirection) -> u8 {
    match direction {
        ScrollDirection::Right => 0x29,
        ScrollDirection::Left => 0x2A,
    }
}

const fn segment_remap_command(remap: SegmentRemap) -> u8 {
    match remap {
        SegmentRemap::Normal => 0xA0,
        SegmentRemap::Remapped => 0xA1,
    }
}

const fn com_scan_direction_command(direction: ComScanDirection) -> u8 {
    match direction {
        ComScanDirection::Normal => 0xC0,
        ComScanDirection::Remapped => 0xC8,
    }
}

const fn scroll_interval_bits(interval: ScrollFrameInterval) -> u8 {
    match interval {
        ScrollFrameInterval::Frames5 => 0x00,
        ScrollFrameInterval::Frames64 => 0x01,
        ScrollFrameInterval::Frames128 => 0x02,
        ScrollFrameInterval::Frames256 => 0x03,
        ScrollFrameInterval::Frames3 => 0x04,
        ScrollFrameInterval::Frames4 => 0x05,
        ScrollFrameInterval::Frames25 => 0x06,
        ScrollFrameInterval::Frames2 => 0x07,
    }
}

const fn orientation_commands(orientation: Orientation) -> [u8; 2] {
    [
        segment_remap_command(orientation.segment_remap),
        com_scan_direction_command(orientation.com_scan_direction),
    ]
}

fn horizontal_scroll_setup<SIZE>(
    direction: ScrollDirection,
    pages: PageRange<SIZE>,
    interval: ScrollFrameInterval,
) -> [u8; 7]
where
    SIZE: DisplaySize,
{
    [
        scroll_command(direction),
        0x00,
        pages.start().as_u8(),
        scroll_interval_bits(interval),
        pages.end().as_u8(),
        0x00,
        0xFF,
    ]
}

fn vertical_scroll_area_setup<SIZE>(area: VerticalScrollArea<SIZE>) -> [u8; 3]
where
    SIZE: DisplaySize,
{
    [
        0xA3,
        area.top_fixed_rows().as_u8(),
        area.scroll_rows().as_u8(),
    ]
}

fn diagonal_scroll_setup<SIZE>(
    direction: ScrollDirection,
    pages: PageRange<SIZE>,
    interval: ScrollFrameInterval,
    vertical_offset: DisplayLine<SIZE>,
) -> [u8; 6]
where
    SIZE: DisplaySize,
{
    [
        diagonal_scroll_command(direction),
        0x00,
        pages.start().as_u8(),
        scroll_interval_bits(interval),
        pages.end().as_u8(),
        vertical_offset.as_u8(),
    ]
}

fn map_init_error<BusError, PinError>(error: Error<BusError>) -> InitError<BusError, PinError> {
    match error {
        Error::Bus(bus) => InitError::Bus(bus),
        Error::InvalidArgument(InvalidArgument::VerticalScrollOffsetOutOfRange) => {
            unreachable!("init does not validate scroll arguments")
        }
    }
}

#[cfg(feature = "graphics")]
impl<DI, SIZE, SCROLL> embedded_graphics_core::geometry::OriginDimensions
    for Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>, SCROLL>
where
    SIZE: DisplaySize,
{
    fn size(&self) -> embedded_graphics_core::geometry::Size {
        embedded_graphics_core::geometry::Size::new(SIZE::WIDTH.into(), SIZE::HEIGHT.into())
    }
}

#[cfg(feature = "graphics")]
impl<DI, SIZE, SCROLL> embedded_graphics_core::draw_target::DrawTarget
    for Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>, SCROLL>
where
    SIZE: DisplaySize,
{
    type Color = embedded_graphics_core::pixelcolor::BinaryColor;
    type Error = Infallible;

    fn draw_iter<PIX>(&mut self, pixels: PIX) -> core::result::Result<(), Self::Error>
    where
        PIX: IntoIterator<
            Item = embedded_graphics_core::Pixel<embedded_graphics_core::pixelcolor::BinaryColor>,
        >,
    {
        use embedded_graphics_core::pixelcolor::BinaryColor;

        for embedded_graphics_core::Pixel(point, color) in pixels {
            if point.x < 0 || point.y < 0 {
                continue;
            }

            self.set_pixel(point.x as u32, point.y as u32, color == BinaryColor::On);
        }

        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> core::result::Result<(), Self::Error> {
        match color {
            embedded_graphics_core::pixelcolor::BinaryColor::Off => self.clear(),
            embedded_graphics_core::pixelcolor::BinaryColor::On => {
                self.mode.buffer.as_mut().fill(0xFF)
            }
        }
        Ok(())
    }
}
