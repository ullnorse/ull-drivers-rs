#![no_std]
#![doc = include_str!("../README.md")]

use core::marker::PhantomData;

#[cfg(feature = "graphics")]
use core::convert::Infallible;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::i2c::{I2c, SevenBitAddress};

const CONTROL_COMMAND: u8 = 0x00;
const CONTROL_DATA: u8 = 0x40;
const CHUNK_SIZE: usize = 16;

/// Driver result type.
pub type Result<T, E> = core::result::Result<T, Error<E>>;

mod private {
    use super::{DisplaySize96x16, DisplaySize128x32, DisplaySize128x64};

    pub trait Sealed {}

    pub trait DisplaySizePrivate {
        fn make_buffer() -> <Self as super::DisplaySize>::Buffer
        where
            Self: super::DisplaySize;
        fn multiplex() -> u8;
        fn com_pins() -> u8;
    }

    impl Sealed for DisplaySize128x64 {}
    impl Sealed for DisplaySize128x32 {}
    impl Sealed for DisplaySize96x16 {}
}

/// SSD1306 I2C address selection.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Address(u8);

impl Address {
    /// `0x3C`, the most common SSD1306 I2C address.
    pub const DEFAULT: Self = Self(0x3C);

    /// `0x3D`, the alternate SSD1306 I2C address.
    pub const ALTERNATE: Self = Self(0x3D);

    /// Creates an address from a supported 7-bit display address.
    #[must_use]
    pub const fn custom(address: u8) -> Option<Self> {
        if address == Self::DEFAULT.0 || address == Self::ALTERNATE.0 {
            Some(Self(address))
        } else {
            None
        }
    }

    /// Returns the 7-bit I2C address.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self.0
    }
}

impl Default for Address {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// SSD1306 page address for the selected panel size.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Page<SIZE> {
    value: u8,
    _size: PhantomData<SIZE>,
}

impl<SIZE> Page<SIZE>
where
    SIZE: DisplaySize,
{
    /// Creates a page index in the panel GDDRAM page range.
    #[must_use]
    pub const fn new(page: u8) -> Option<Self> {
        if page < (SIZE::HEIGHT / 8) {
            Some(Self {
                value: page,
                _size: PhantomData,
            })
        } else {
            None
        }
    }

    /// Returns the first valid page for the selected panel size.
    #[must_use]
    pub const fn first() -> Self {
        Self {
            value: 0,
            _size: PhantomData,
        }
    }

    /// Returns the last valid page for the selected panel size.
    #[must_use]
    pub const fn last() -> Self {
        Self {
            value: (SIZE::HEIGHT / 8) - 1,
            _size: PhantomData,
        }
    }

    /// Returns the raw page index.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self.value
    }
}

/// Inclusive SSD1306 page range used by hardware scrolling commands.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PageRange<SIZE> {
    start: Page<SIZE>,
    end: Page<SIZE>,
}

impl<SIZE> PageRange<SIZE>
where
    SIZE: DisplaySize,
{
    /// Creates an inclusive page range with `start <= end`.
    #[must_use]
    pub const fn new(start: Page<SIZE>, end: Page<SIZE>) -> Option<Self> {
        if start.as_u8() <= end.as_u8() {
            Some(Self { start, end })
        } else {
            None
        }
    }

    /// Returns a range that covers the whole panel height.
    #[must_use]
    pub const fn whole_display() -> Self {
        Self {
            start: Page::first(),
            end: Page::last(),
        }
    }

    /// Returns the starting page.
    #[must_use]
    pub const fn start(self) -> Page<SIZE> {
        self.start
    }

    /// Returns the ending page.
    #[must_use]
    pub const fn end(self) -> Page<SIZE> {
        self.end
    }
}

/// SSD1306 display line / vertical offset for the selected panel size.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DisplayLine<SIZE> {
    value: u8,
    _size: PhantomData<SIZE>,
}

impl<SIZE> DisplayLine<SIZE>
where
    SIZE: DisplaySize,
{
    /// Creates a display line or vertical offset value.
    #[must_use]
    pub const fn new(line: u8) -> Option<Self> {
        if line < SIZE::HEIGHT {
            Some(Self {
                value: line,
                _size: PhantomData,
            })
        } else {
            None
        }
    }

    /// Returns line 0.
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            value: 0,
            _size: PhantomData,
        }
    }

    /// Returns the raw line value.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self.value
    }
}

/// Raw SSD1306 display-offset register value in COM lines.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DisplayOffset {
    value: u8,
}

impl DisplayOffset {
    /// Creates a display-offset value in the inclusive range `0..=63`.
    #[must_use]
    pub const fn new(offset: u8) -> Option<Self> {
        if offset <= 63 {
            Some(Self { value: offset })
        } else {
            None
        }
    }

    /// Returns offset 0.
    #[must_use]
    pub const fn zero() -> Self {
        Self { value: 0 }
    }

    /// Returns the raw display-offset register value.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self.value
    }
}

/// Number of panel rows for the selected display size.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RowCount<SIZE> {
    value: u8,
    _size: PhantomData<SIZE>,
}

impl<SIZE> RowCount<SIZE>
where
    SIZE: DisplaySize,
{
    /// Creates a row count in the inclusive range `0..=HEIGHT`.
    #[must_use]
    pub const fn new(rows: u8) -> Option<Self> {
        if rows <= SIZE::HEIGHT {
            Some(Self {
                value: rows,
                _size: PhantomData,
            })
        } else {
            None
        }
    }

    /// Returns a zero-row count.
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            value: 0,
            _size: PhantomData,
        }
    }

    /// Returns a row count covering the whole panel height.
    #[must_use]
    pub const fn whole_display() -> Self {
        Self {
            value: SIZE::HEIGHT,
            _size: PhantomData,
        }
    }

    /// Returns the raw row count.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self.value
    }
}

/// Vertical scroll area configuration for continuous diagonal scrolling.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VerticalScrollArea<SIZE> {
    top_fixed_rows: RowCount<SIZE>,
    scroll_rows: RowCount<SIZE>,
}

impl<SIZE> VerticalScrollArea<SIZE>
where
    SIZE: DisplaySize,
{
    /// Creates a vertical scroll area that satisfies the SSD1306 MUX limits.
    #[must_use]
    pub const fn new(top_fixed_rows: RowCount<SIZE>, scroll_rows: RowCount<SIZE>) -> Option<Self> {
        let top = top_fixed_rows.as_u8();
        let scroll = scroll_rows.as_u8();

        if scroll > 0 && top + scroll <= SIZE::HEIGHT {
            Some(Self {
                top_fixed_rows,
                scroll_rows,
            })
        } else {
            None
        }
    }

    /// Returns a scroll area that covers the whole panel.
    #[must_use]
    pub const fn whole_display() -> Self {
        Self {
            top_fixed_rows: RowCount::zero(),
            scroll_rows: RowCount::whole_display(),
        }
    }

    /// Returns the top fixed row count.
    #[must_use]
    pub const fn top_fixed_rows(self) -> RowCount<SIZE> {
        self.top_fixed_rows
    }

    /// Returns the scrolling row count.
    #[must_use]
    pub const fn scroll_rows(self) -> RowCount<SIZE> {
        self.scroll_rows
    }

    /// Returns whether a vertical offset is valid for this scroll area.
    #[must_use]
    pub const fn supports_offset(self, offset: DisplayLine<SIZE>) -> bool {
        offset.as_u8() < self.scroll_rows.as_u8()
    }
}

/// Display rotation.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Rotation {
    /// Segment 0 at column 0, COM 0 at row 0.
    #[default]
    Rotate0,
    /// Segment 127 at column 0, COM N at row 0.
    Rotate180,
}

/// SSD1306 segment mapping configuration.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SegmentRemap {
    /// Column address 0 is mapped to SEG0.
    #[default]
    Normal,
    /// Column address 127 is mapped to SEG0.
    Remapped,
}

/// SSD1306 COM output scan direction.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ComScanDirection {
    /// Scan from COM0 to COM[N-1].
    #[default]
    Normal,
    /// Scan from COM[N-1] to COM0.
    Remapped,
}

/// Full display orientation as a combination of segment and COM remapping.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Orientation {
    /// Segment remap configuration.
    pub segment_remap: SegmentRemap,
    /// COM output scan direction.
    pub com_scan_direction: ComScanDirection,
}

impl Orientation {
    /// No mirroring: column 0 -> SEG0, COM0 -> row 0.
    pub const ROTATE_0: Self = Self {
        segment_remap: SegmentRemap::Normal,
        com_scan_direction: ComScanDirection::Normal,
    };

    /// Mirror both axes: column 127 -> SEG0, COM[N-1] -> row 0.
    pub const ROTATE_180: Self = Self {
        segment_remap: SegmentRemap::Remapped,
        com_scan_direction: ComScanDirection::Remapped,
    };

    /// Returns the matching convenience rotation, if one exists.
    #[must_use]
    pub const fn rotation(self) -> Option<Rotation> {
        match (self.segment_remap, self.com_scan_direction) {
            (SegmentRemap::Normal, ComScanDirection::Normal) => Some(Rotation::Rotate0),
            (SegmentRemap::Remapped, ComScanDirection::Remapped) => Some(Rotation::Rotate180),
            _ => None,
        }
    }
}

impl From<Rotation> for Orientation {
    fn from(rotation: Rotation) -> Self {
        match rotation {
            Rotation::Rotate0 => Self::ROTATE_0,
            Rotation::Rotate180 => Self::ROTATE_180,
        }
    }
}

/// Panel power source selection for initialization defaults.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PowerSource {
    /// Use the SSD1306 internal charge pump.
    #[default]
    Internal,
    /// Use an externally generated panel voltage.
    External,
}

/// Horizontal hardware scroll direction.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ScrollDirection {
    /// Scroll toward increasing column addresses on screen.
    Right,
    /// Scroll toward decreasing column addresses on screen.
    Left,
}

/// Hardware scroll step interval in display frames.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ScrollFrameInterval {
    Frames2,
    Frames3,
    Frames4,
    Frames5,
    Frames25,
    Frames64,
    Frames128,
    Frames256,
}

/// Initialization options applied by [`Ssd1306::init_with_config`].
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Config {
    /// Display orientation.
    pub orientation: Orientation,
    /// Charge pump / panel supply configuration.
    pub power_source: PowerSource,
    /// Initial contrast register value.
    pub contrast: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            orientation: Orientation::default(),
            power_source: PowerSource::default(),
            contrast: 0x7F,
        }
    }
}

/// Invalid API arguments rejected by the SSD1306 driver.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum InvalidArgument {
    /// The diagonal scroll vertical offset must fit within the configured scrolling rows.
    VerticalScrollOffsetOutOfRange,
}

/// Errors returned by the SSD1306 driver.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Error<BusError> {
    /// Bus error while talking to the display.
    Bus(BusError),
    /// Invalid API arguments.
    InvalidArgument(InvalidArgument),
}

impl<BusError> core::fmt::Display for Error<BusError>
where
    BusError: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bus(error) => write!(f, "display bus error: {error:?}"),
            Self::InvalidArgument(InvalidArgument::VerticalScrollOffsetOutOfRange) => {
                write!(
                    f,
                    "vertical scroll offset exceeds configured scrolling rows"
                )
            }
        }
    }
}

/// Errors returned by reset-aware initialization helpers.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum InitError<BusError, PinError> {
    /// Bus error while talking to the display.
    Bus(BusError),
    /// Error while driving the hardware reset pin.
    ResetPin(PinError),
}

impl<BusError, PinError> core::fmt::Display for InitError<BusError, PinError>
where
    BusError: core::fmt::Debug,
    PinError: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bus(error) => write!(f, "display bus error: {error:?}"),
            Self::ResetPin(error) => write!(f, "display reset pin error: {error:?}"),
        }
    }
}

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

/// Typed SSD1306 panel geometry.
pub trait DisplaySize: Copy + private::Sealed + private::DisplaySizePrivate {
    /// Panel width in pixels.
    const WIDTH: u8;
    /// Panel height in pixels.
    const HEIGHT: u8;
    /// Framebuffer storage type.
    type Buffer: AsRef<[u8]> + AsMut<[u8]>;
}

/// 128x64 SSD1306 panel.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct DisplaySize128x64;

impl DisplaySize for DisplaySize128x64 {
    const WIDTH: u8 = 128;
    const HEIGHT: u8 = 64;
    type Buffer = [u8; 1024];
}

impl private::DisplaySizePrivate for DisplaySize128x64 {
    fn make_buffer() -> <Self as DisplaySize>::Buffer {
        [0; 1024]
    }
    fn multiplex() -> u8 {
        0x3F
    }

    fn com_pins() -> u8 {
        0x12
    }
}

/// 128x32 SSD1306 panel.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct DisplaySize128x32;

impl DisplaySize for DisplaySize128x32 {
    const WIDTH: u8 = 128;
    const HEIGHT: u8 = 32;
    type Buffer = [u8; 512];
}

impl private::DisplaySizePrivate for DisplaySize128x32 {
    fn make_buffer() -> <Self as DisplaySize>::Buffer {
        [0; 512]
    }
    fn multiplex() -> u8 {
        0x1F
    }

    fn com_pins() -> u8 {
        0x02
    }
}

/// 96x16 SSD1306 panel.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct DisplaySize96x16;

impl DisplaySize for DisplaySize96x16 {
    const WIDTH: u8 = 96;
    const HEIGHT: u8 = 16;
    type Buffer = [u8; 192];
}

impl private::DisplaySizePrivate for DisplaySize96x16 {
    fn make_buffer() -> <Self as DisplaySize>::Buffer {
        [0; 192]
    }
    fn multiplex() -> u8 {
        0x0F
    }

    fn com_pins() -> u8 {
        0x02
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

impl<DI, SIZE> Ssd1306<DI, SIZE, RawMode, ScrollInactive>
where
    DI: I2c<SevenBitAddress>,
    SIZE: DisplaySize,
{
    /// Updates the display rotation.
    pub fn set_rotation(&mut self, rotation: Rotation) -> Result<(), DI::Error> {
        self.set_orientation(rotation.into())
    }

    /// Updates the full display orientation.
    pub fn set_orientation(&mut self, orientation: Orientation) -> Result<(), DI::Error> {
        self.apply_orientation(orientation)?;
        self.config.orientation = orientation;
        Ok(())
    }

    /// Updates only the segment remap configuration.
    pub fn set_segment_remap(&mut self, segment_remap: SegmentRemap) -> Result<(), DI::Error> {
        let mut orientation = self.config.orientation;
        orientation.segment_remap = segment_remap;
        self.set_orientation(orientation)
    }

    /// Updates only the COM scan direction.
    pub fn set_com_scan_direction(
        &mut self,
        com_scan_direction: ComScanDirection,
    ) -> Result<(), DI::Error> {
        let mut orientation = self.config.orientation;
        orientation.com_scan_direction = com_scan_direction;
        self.set_orientation(orientation)
    }

    /// Sends a single raw SSD1306 command byte.
    pub fn write_command(&mut self, command: u8) -> Result<(), DI::Error> {
        self.send_commands(&[command])
    }

    /// Sends one or more raw SSD1306 command bytes.
    pub fn write_commands(&mut self, commands: &[u8]) -> Result<(), DI::Error> {
        self.send_commands(commands)
    }

    /// Sends raw display RAM bytes using the current controller addressing mode.
    pub fn write_data(&mut self, data: &[u8]) -> Result<(), DI::Error> {
        self.send_data(data)
    }
}

#[cfg(feature = "async")]
impl<DI, SIZE> Ssd1306<DI, SIZE, RawMode, ScrollInactive>
where
    DI: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
    SIZE: DisplaySize,
{
    /// Async version of [`Self::set_rotation`].
    pub async fn set_rotation_async(&mut self, rotation: Rotation) -> Result<(), DI::Error> {
        self.set_orientation_async(rotation.into()).await
    }

    /// Async version of [`Self::set_orientation`].
    pub async fn set_orientation_async(
        &mut self,
        orientation: Orientation,
    ) -> Result<(), DI::Error> {
        self.apply_orientation_async(orientation).await?;
        self.config.orientation = orientation;
        Ok(())
    }

    /// Async version of [`Self::set_segment_remap`].
    pub async fn set_segment_remap_async(
        &mut self,
        segment_remap: SegmentRemap,
    ) -> Result<(), DI::Error> {
        let mut orientation = self.config.orientation;
        orientation.segment_remap = segment_remap;
        self.set_orientation_async(orientation).await
    }

    /// Async version of [`Self::set_com_scan_direction`].
    pub async fn set_com_scan_direction_async(
        &mut self,
        com_scan_direction: ComScanDirection,
    ) -> Result<(), DI::Error> {
        let mut orientation = self.config.orientation;
        orientation.com_scan_direction = com_scan_direction;
        self.set_orientation_async(orientation).await
    }

    /// Async version of [`Self::write_command`].
    pub async fn write_command_async(&mut self, command: u8) -> Result<(), DI::Error> {
        self.send_commands_async(&[command]).await
    }

    /// Async version of [`Self::write_commands`].
    pub async fn write_commands_async(&mut self, commands: &[u8]) -> Result<(), DI::Error> {
        self.send_commands_async(commands).await
    }

    /// Async version of [`Self::write_data`].
    pub async fn write_data_async(&mut self, data: &[u8]) -> Result<(), DI::Error> {
        self.send_data_async(data).await
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

impl<DI, SIZE> Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>, ScrollInactive>
where
    DI: I2c<SevenBitAddress>,
    SIZE: DisplaySize,
{
    /// Uploads the full framebuffer to display RAM.
    pub fn flush(&mut self) -> Result<(), DI::Error> {
        self.set_horizontal_addressing_mode()?;
        self.set_draw_area(0, SIZE::WIDTH - 1, 0, (SIZE::HEIGHT / 8) - 1)?;
        let address = self.address;
        let i2c = &mut self.i2c;
        let buffer = self.mode.buffer.as_ref();
        send_frame(i2c, address, CONTROL_DATA, buffer)
    }

    /// Uploads a framebuffer sub-area to display RAM.
    ///
    /// SSD1306 RAM is page-oriented, so the vertical range is rounded out to the
    /// affected 8-pixel pages.
    pub fn flush_area(&mut self, x: u32, y: u32, width: u32, height: u32) -> Result<(), DI::Error> {
        let max_width = u32::from(SIZE::WIDTH);
        let max_height = u32::from(SIZE::HEIGHT);
        let end_x = x.saturating_add(width).min(max_width);
        let end_y = y.saturating_add(height).min(max_height);

        if x >= end_x || y >= end_y {
            return Ok(());
        }

        let start_column = x as u8;
        let end_column = (end_x - 1) as u8;
        let start_page = (y / 8) as u8;
        let end_page = ((end_y - 1) / 8) as u8;

        self.set_horizontal_addressing_mode()?;
        self.set_draw_area(start_column, end_column, start_page, end_page)?;

        let page_width = usize::from(SIZE::WIDTH);
        let start_column = usize::from(start_column);
        let end_column = usize::from(end_column) + 1;
        let i2c = &mut self.i2c;
        let buffer = self.mode.buffer.as_ref();

        for page in start_page..=end_page {
            let page_offset = usize::from(page) * page_width;
            send_frame(
                i2c,
                self.address,
                CONTROL_DATA,
                &buffer[page_offset + start_column..page_offset + end_column],
            )?;
        }

        Ok(())
    }
}

impl<DI, SIZE> Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>, ScrollRestoreRequired>
where
    DI: I2c<SevenBitAddress>,
    SIZE: DisplaySize,
{
    /// Rewrites the framebuffer after stopping hardware scroll, then returns to the inactive state.
    pub fn restore_display(
        mut self,
    ) -> StateChangeResult<
        Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>, ScrollInactive>,
        Self,
        DI::Error,
    > {
        let result = (|| {
            self.set_horizontal_addressing_mode()?;
            self.set_draw_area(0, SIZE::WIDTH - 1, 0, (SIZE::HEIGHT / 8) - 1)?;
            send_frame(
                &mut self.i2c,
                self.address,
                CONTROL_DATA,
                self.mode.buffer.as_ref(),
            )
        })();

        if let Err(error) = result {
            return Err(StateChangeError::new(self, error));
        }

        Ok(Ssd1306 {
            i2c: self.i2c,
            address: self.address,
            mode: self.mode,
            config: self.config,
            _size: PhantomData,
            _scroll: PhantomData,
        })
    }
}

impl<DI, SIZE> Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>, ScrollInactive>
where
    DI: I2c<SevenBitAddress>,
    SIZE: DisplaySize,
{
    /// Updates the display rotation.
    pub fn set_rotation(&mut self, rotation: Rotation) -> Result<(), DI::Error> {
        self.set_orientation(rotation.into())
    }

    /// Updates the full display orientation.
    pub fn set_orientation(&mut self, orientation: Orientation) -> Result<(), DI::Error> {
        self.apply_orientation(orientation)?;
        self.config.orientation = orientation;
        Ok(())
    }

    /// Updates only the segment remap configuration.
    pub fn set_segment_remap(&mut self, segment_remap: SegmentRemap) -> Result<(), DI::Error> {
        let mut orientation = self.config.orientation;
        orientation.segment_remap = segment_remap;
        self.set_orientation(orientation)
    }

    /// Updates only the COM scan direction.
    pub fn set_com_scan_direction(
        &mut self,
        com_scan_direction: ComScanDirection,
    ) -> Result<(), DI::Error> {
        let mut orientation = self.config.orientation;
        orientation.com_scan_direction = com_scan_direction;
        self.set_orientation(orientation)
    }
}

#[cfg(feature = "async")]
impl<DI, SIZE> Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>, ScrollInactive>
where
    DI: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
    SIZE: DisplaySize,
{
    /// Async version of [`Self::flush`].
    pub async fn flush_async(&mut self) -> Result<(), DI::Error> {
        self.set_horizontal_addressing_mode_async().await?;
        self.set_draw_area_async(0, SIZE::WIDTH - 1, 0, (SIZE::HEIGHT / 8) - 1)
            .await?;
        let address = self.address;
        let i2c = &mut self.i2c;
        let buffer = self.mode.buffer.as_ref();
        send_frame_async(i2c, address, CONTROL_DATA, buffer).await
    }

    /// Async version of [`Self::flush_area`].
    pub async fn flush_area_async(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), DI::Error> {
        let max_width = u32::from(SIZE::WIDTH);
        let max_height = u32::from(SIZE::HEIGHT);
        let end_x = x.saturating_add(width).min(max_width);
        let end_y = y.saturating_add(height).min(max_height);

        if x >= end_x || y >= end_y {
            return Ok(());
        }

        let start_column = x as u8;
        let end_column = (end_x - 1) as u8;
        let start_page = (y / 8) as u8;
        let end_page = ((end_y - 1) / 8) as u8;

        self.set_horizontal_addressing_mode_async().await?;
        self.set_draw_area_async(start_column, end_column, start_page, end_page)
            .await?;

        let page_width = usize::from(SIZE::WIDTH);
        let start_column = usize::from(start_column);
        let end_column = usize::from(end_column) + 1;
        let i2c = &mut self.i2c;
        let buffer = self.mode.buffer.as_ref();

        for page in start_page..=end_page {
            let page_offset = usize::from(page) * page_width;
            send_frame_async(
                i2c,
                self.address,
                CONTROL_DATA,
                &buffer[page_offset + start_column..page_offset + end_column],
            )
            .await?;
        }

        Ok(())
    }

    /// Async version of [`Self::set_rotation`].
    pub async fn set_rotation_async(&mut self, rotation: Rotation) -> Result<(), DI::Error> {
        self.set_orientation_async(rotation.into()).await
    }

    /// Async version of [`Self::set_orientation`].
    pub async fn set_orientation_async(
        &mut self,
        orientation: Orientation,
    ) -> Result<(), DI::Error> {
        self.apply_orientation_async(orientation).await?;
        self.config.orientation = orientation;
        Ok(())
    }

    /// Async version of [`Self::set_segment_remap`].
    pub async fn set_segment_remap_async(
        &mut self,
        segment_remap: SegmentRemap,
    ) -> Result<(), DI::Error> {
        let mut orientation = self.config.orientation;
        orientation.segment_remap = segment_remap;
        self.set_orientation_async(orientation).await
    }

    /// Async version of [`Self::set_com_scan_direction`].
    pub async fn set_com_scan_direction_async(
        &mut self,
        com_scan_direction: ComScanDirection,
    ) -> Result<(), DI::Error> {
        let mut orientation = self.config.orientation;
        orientation.com_scan_direction = com_scan_direction;
        self.set_orientation_async(orientation).await
    }
}

#[cfg(feature = "async")]
impl<DI, SIZE> Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>, ScrollRestoreRequired>
where
    DI: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
    SIZE: DisplaySize,
{
    /// Rewrites the framebuffer after stopping hardware scroll, then returns to the inactive state.
    pub async fn restore_display_async(
        mut self,
    ) -> StateChangeResult<
        Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>, ScrollInactive>,
        Self,
        DI::Error,
    > {
        let result = async {
            self.set_horizontal_addressing_mode_async().await?;
            self.set_draw_area_async(0, SIZE::WIDTH - 1, 0, (SIZE::HEIGHT / 8) - 1)
                .await?;
            send_frame_async(
                &mut self.i2c,
                self.address,
                CONTROL_DATA,
                self.mode.buffer.as_ref(),
            )
            .await
        }
        .await;

        if let Err(error) = result {
            return Err(StateChangeError::new(self, error));
        }

        Ok(Ssd1306 {
            i2c: self.i2c,
            address: self.address,
            mode: self.mode,
            config: self.config,
            _size: PhantomData,
            _scroll: PhantomData,
        })
    }
}

impl<DI, SIZE, MODE> Ssd1306<DI, SIZE, MODE, ScrollInactive>
where
    DI: I2c<SevenBitAddress>,
    SIZE: DisplaySize,
{
    /// Initializes the controller using panel geometry defaults and the SSD1306
    /// datasheet/app-note default contrast (`0x7F`).
    pub fn init(&mut self) -> Result<(), DI::Error> {
        let config = Config {
            orientation: self.config.orientation,
            ..Config::default()
        };
        self.init_with_config(config)
    }

    /// Pulses the hardware reset pin, waits the datasheet minimum delay, then
    /// initializes using panel geometry defaults and contrast `0x7F`.
    pub fn init_with_reset<RST, DELAY>(
        &mut self,
        reset: &mut RST,
        delay: &mut DELAY,
    ) -> core::result::Result<(), InitError<DI::Error, RST::Error>>
    where
        RST: OutputPin,
        DELAY: DelayNs,
    {
        let config = Config {
            orientation: self.config.orientation,
            ..Config::default()
        };
        self.init_with_config_and_reset(config, reset, delay)
    }

    /// Initializes the controller with an explicit configuration.
    pub fn init_with_config(&mut self, config: Config) -> Result<(), DI::Error> {
        self.send_commands(&[
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
        ])?;

        self.apply_orientation(config.orientation)?;
        self.send_commands(&[
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
        ])?;

        self.config = config;
        Ok(())
    }

    /// Pulses the hardware reset pin, waits the datasheet minimum delay, then
    /// initializes the controller with an explicit configuration.
    pub fn init_with_config_and_reset<RST, DELAY>(
        &mut self,
        config: Config,
        reset: &mut RST,
        delay: &mut DELAY,
    ) -> core::result::Result<(), InitError<DI::Error, RST::Error>>
    where
        RST: OutputPin,
        DELAY: DelayNs,
    {
        reset.set_low().map_err(InitError::ResetPin)?;
        delay.delay_us(3);
        reset.set_high().map_err(InitError::ResetPin)?;
        delay.delay_us(3);
        self.init_with_config(config).map_err(|error| match error {
            Error::Bus(bus) => InitError::Bus(bus),
            Error::InvalidArgument(InvalidArgument::VerticalScrollOffsetOutOfRange) => {
                unreachable!("init does not validate scroll arguments")
            }
        })
    }
}

impl<DI, SIZE, MODE, SCROLL> Ssd1306<DI, SIZE, MODE, SCROLL>
where
    DI: I2c<SevenBitAddress>,
    SIZE: DisplaySize,
{
    /// Enables or disables display output.
    pub fn set_display_on(&mut self, on: bool) -> Result<(), DI::Error> {
        self.send_command(if on { 0xAF } else { 0xAE })
    }

    /// Enables or disables the entire display output, ignoring GDDRAM while enabled.
    pub fn set_entire_display_on(&mut self, enabled: bool) -> Result<(), DI::Error> {
        self.send_command(if enabled { 0xA5 } else { 0xA4 })
    }

    /// Enables or disables display inversion.
    pub fn set_invert(&mut self, inverted: bool) -> Result<(), DI::Error> {
        self.send_command(if inverted { 0xA7 } else { 0xA6 })
    }

    /// Sets display contrast.
    pub fn set_contrast(&mut self, contrast: u8) -> Result<(), DI::Error> {
        self.send_commands(&[0x81, contrast])
    }

    /// Sets the display start line within the configured panel line range (`40h..=7Fh`).
    pub fn set_display_start_line(&mut self, line: DisplayLine<SIZE>) -> Result<(), DI::Error> {
        self.send_command(0x40 | line.as_u8())
    }

    /// Sets the vertical display offset (`D3h`).
    pub fn set_display_offset(&mut self, offset: DisplayOffset) -> Result<(), DI::Error> {
        self.send_commands(&[0xD3, offset.as_u8()])
    }

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

    fn send_command(&mut self, command: u8) -> Result<(), DI::Error> {
        self.send_commands(&[command])
    }

    fn send_commands(&mut self, commands: &[u8]) -> Result<(), DI::Error> {
        send_frame(&mut self.i2c, self.address, CONTROL_COMMAND, commands)
    }

    fn send_data(&mut self, data: &[u8]) -> Result<(), DI::Error> {
        send_frame(&mut self.i2c, self.address, CONTROL_DATA, data)
    }

    fn apply_orientation(&mut self, orientation: Orientation) -> Result<(), DI::Error> {
        self.send_commands(&[
            segment_remap_command(orientation.segment_remap),
            com_scan_direction_command(orientation.com_scan_direction),
        ])
    }

    fn set_horizontal_addressing_mode(&mut self) -> Result<(), DI::Error> {
        self.send_commands(&[0x20, 0x00])
    }

    fn set_draw_area(
        &mut self,
        start_column: u8,
        end_column: u8,
        start_page: u8,
        end_page: u8,
    ) -> Result<(), DI::Error> {
        self.send_commands(&[0x21, start_column, end_column, 0x22, start_page, end_page])
    }

    fn set_vertical_scroll_area_inner(
        &mut self,
        area: VerticalScrollArea<SIZE>,
    ) -> Result<(), DI::Error> {
        self.send_commands(&[
            0xA3,
            area.top_fixed_rows().as_u8(),
            area.scroll_rows().as_u8(),
        ])
    }
}

impl<DI, SIZE, MODE> Ssd1306<DI, SIZE, MODE, ScrollInactive>
where
    DI: I2c<SevenBitAddress>,
    SIZE: DisplaySize,
{
    /// Configures a hardware horizontal scroll operation.
    pub fn configure_horizontal_scroll(
        &mut self,
        direction: ScrollDirection,
        pages: PageRange<SIZE>,
        interval: ScrollFrameInterval,
    ) -> Result<(), DI::Error> {
        self.send_command(0x2E)?;
        self.send_commands(&[
            scroll_command(direction),
            0x00,
            pages.start().as_u8(),
            scroll_interval_bits(interval),
            pages.end().as_u8(),
            0x00,
            0xFF,
        ])
    }

    /// Programs the vertical scroll area used by continuous diagonal scrolling.
    pub fn set_vertical_scroll_area(
        &mut self,
        area: VerticalScrollArea<SIZE>,
    ) -> Result<(), DI::Error> {
        self.send_command(0x2E)?;
        self.set_vertical_scroll_area_inner(area)
    }

    /// Configures a hardware diagonal scroll operation, including the vertical scroll area.
    pub fn configure_diagonal_scroll(
        &mut self,
        direction: ScrollDirection,
        pages: PageRange<SIZE>,
        interval: ScrollFrameInterval,
        area: VerticalScrollArea<SIZE>,
        vertical_offset: DisplayLine<SIZE>,
    ) -> Result<(), DI::Error> {
        if !area.supports_offset(vertical_offset) {
            return Err(Error::InvalidArgument(
                InvalidArgument::VerticalScrollOffsetOutOfRange,
            ));
        }

        self.send_command(0x2E)?;
        self.set_vertical_scroll_area_inner(area)?;
        self.send_commands(&[
            diagonal_scroll_command(direction),
            0x00,
            pages.start().as_u8(),
            scroll_interval_bits(interval),
            pages.end().as_u8(),
            vertical_offset.as_u8(),
        ])
    }

    /// Activates the previously configured hardware scroll operation.
    pub fn start_scroll(
        mut self,
    ) -> StateChangeResult<Ssd1306<DI, SIZE, MODE, ScrollActive>, Self, DI::Error> {
        if let Err(error) = self.send_command(0x2F) {
            return Err(StateChangeError::new(self, error));
        }

        Ok(Ssd1306 {
            i2c: self.i2c,
            address: self.address,
            mode: self.mode,
            config: self.config,
            _size: PhantomData,
            _scroll: PhantomData,
        })
    }
}

impl<DI, SIZE, MODE> Ssd1306<DI, SIZE, MODE, ScrollActive>
where
    DI: I2c<SevenBitAddress>,
    SIZE: DisplaySize,
{
    /// Deactivates hardware scrolling and enters a state that requires a RAM rewrite.
    pub fn stop_scroll(
        mut self,
    ) -> StateChangeResult<Ssd1306<DI, SIZE, MODE, ScrollRestoreRequired>, Self, DI::Error> {
        if let Err(error) = self.send_command(0x2E) {
            return Err(StateChangeError::new(self, error));
        }

        Ok(Ssd1306 {
            i2c: self.i2c,
            address: self.address,
            mode: self.mode,
            config: self.config,
            _size: PhantomData,
            _scroll: PhantomData,
        })
    }
}

impl<DI, SIZE, MODE> Ssd1306<DI, SIZE, MODE, ScrollRestoreRequired>
where
    SIZE: DisplaySize,
{
    /// Marks scroll restoration complete after application code has rewritten GDDRAM.
    #[must_use]
    pub fn finish_scroll_rewrite(self) -> Ssd1306<DI, SIZE, MODE, ScrollInactive> {
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

#[cfg(feature = "async")]
impl<DI, SIZE, MODE> Ssd1306<DI, SIZE, MODE, ScrollInactive>
where
    DI: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
    SIZE: DisplaySize,
{
    /// Async version of [`Self::init`].
    pub async fn init_async(&mut self) -> Result<(), DI::Error> {
        let config = Config {
            orientation: self.config.orientation,
            ..Config::default()
        };
        self.init_with_config_async(config).await
    }

    /// Async version of [`Self::init_with_reset`].
    pub async fn init_with_reset_async<RST, DELAY>(
        &mut self,
        reset: &mut RST,
        delay: &mut DELAY,
    ) -> core::result::Result<(), InitError<DI::Error, RST::Error>>
    where
        RST: OutputPin,
        DELAY: embedded_hal_async::delay::DelayNs,
    {
        let config = Config {
            orientation: self.config.orientation,
            ..Config::default()
        };
        self.init_with_config_and_reset_async(config, reset, delay)
            .await
    }

    /// Async version of [`Self::init_with_config`].
    pub async fn init_with_config_async(&mut self, config: Config) -> Result<(), DI::Error> {
        self.send_commands_async(&[
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
        ])
        .await?;

        self.apply_orientation_async(config.orientation).await?;
        self.send_commands_async(&[
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
        ])
        .await?;

        self.config = config;
        Ok(())
    }

    /// Async version of [`Self::init_with_config_and_reset`].
    pub async fn init_with_config_and_reset_async<RST, DELAY>(
        &mut self,
        config: Config,
        reset: &mut RST,
        delay: &mut DELAY,
    ) -> core::result::Result<(), InitError<DI::Error, RST::Error>>
    where
        RST: OutputPin,
        DELAY: embedded_hal_async::delay::DelayNs,
    {
        reset.set_low().map_err(InitError::ResetPin)?;
        delay.delay_us(3).await;
        reset.set_high().map_err(InitError::ResetPin)?;
        delay.delay_us(3).await;
        self.init_with_config_async(config)
            .await
            .map_err(|error| match error {
                Error::Bus(bus) => InitError::Bus(bus),
                Error::InvalidArgument(InvalidArgument::VerticalScrollOffsetOutOfRange) => {
                    unreachable!("init does not validate scroll arguments")
                }
            })
    }
}

#[cfg(feature = "async")]
impl<DI, SIZE, MODE, SCROLL> Ssd1306<DI, SIZE, MODE, SCROLL>
where
    DI: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
    SIZE: DisplaySize,
{
    /// Async version of [`Self::set_display_on`].
    pub async fn set_display_on_async(&mut self, on: bool) -> Result<(), DI::Error> {
        self.send_command_async(if on { 0xAF } else { 0xAE }).await
    }

    /// Async version of [`Self::set_entire_display_on`].
    pub async fn set_entire_display_on_async(&mut self, enabled: bool) -> Result<(), DI::Error> {
        self.send_command_async(if enabled { 0xA5 } else { 0xA4 })
            .await
    }

    /// Async version of [`Self::set_invert`].
    pub async fn set_invert_async(&mut self, inverted: bool) -> Result<(), DI::Error> {
        self.send_command_async(if inverted { 0xA7 } else { 0xA6 })
            .await
    }

    /// Async version of [`Self::set_contrast`].
    pub async fn set_contrast_async(&mut self, contrast: u8) -> Result<(), DI::Error> {
        self.send_commands_async(&[0x81, contrast]).await
    }

    /// Async version of [`Self::set_display_start_line`].
    pub async fn set_display_start_line_async(
        &mut self,
        line: DisplayLine<SIZE>,
    ) -> Result<(), DI::Error> {
        self.send_command_async(0x40 | line.as_u8()).await
    }

    /// Async version of [`Self::set_display_offset`].
    pub async fn set_display_offset_async(
        &mut self,
        offset: DisplayOffset,
    ) -> Result<(), DI::Error> {
        self.send_commands_async(&[0xD3, offset.as_u8()]).await
    }

    async fn send_command_async(&mut self, command: u8) -> Result<(), DI::Error> {
        self.send_commands_async(&[command]).await
    }

    async fn send_commands_async(&mut self, commands: &[u8]) -> Result<(), DI::Error> {
        send_frame_async(&mut self.i2c, self.address, CONTROL_COMMAND, commands).await
    }

    async fn send_data_async(&mut self, data: &[u8]) -> Result<(), DI::Error> {
        send_frame_async(&mut self.i2c, self.address, CONTROL_DATA, data).await
    }

    async fn apply_orientation_async(&mut self, orientation: Orientation) -> Result<(), DI::Error> {
        self.send_commands_async(&[
            segment_remap_command(orientation.segment_remap),
            com_scan_direction_command(orientation.com_scan_direction),
        ])
        .await
    }

    async fn set_horizontal_addressing_mode_async(&mut self) -> Result<(), DI::Error> {
        self.send_commands_async(&[0x20, 0x00]).await
    }

    async fn set_draw_area_async(
        &mut self,
        start_column: u8,
        end_column: u8,
        start_page: u8,
        end_page: u8,
    ) -> Result<(), DI::Error> {
        self.send_commands_async(&[0x21, start_column, end_column, 0x22, start_page, end_page])
            .await
    }

    async fn set_vertical_scroll_area_inner_async(
        &mut self,
        area: VerticalScrollArea<SIZE>,
    ) -> Result<(), DI::Error> {
        self.send_commands_async(&[
            0xA3,
            area.top_fixed_rows().as_u8(),
            area.scroll_rows().as_u8(),
        ])
        .await
    }
}

#[cfg(feature = "async")]
impl<DI, SIZE, MODE> Ssd1306<DI, SIZE, MODE, ScrollInactive>
where
    DI: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
    SIZE: DisplaySize,
{
    /// Async version of [`Self::configure_horizontal_scroll`].
    pub async fn configure_horizontal_scroll_async(
        &mut self,
        direction: ScrollDirection,
        pages: PageRange<SIZE>,
        interval: ScrollFrameInterval,
    ) -> Result<(), DI::Error> {
        self.send_command_async(0x2E).await?;
        self.send_commands_async(&[
            scroll_command(direction),
            0x00,
            pages.start().as_u8(),
            scroll_interval_bits(interval),
            pages.end().as_u8(),
            0x00,
            0xFF,
        ])
        .await
    }

    /// Async version of [`Self::set_vertical_scroll_area`].
    pub async fn set_vertical_scroll_area_async(
        &mut self,
        area: VerticalScrollArea<SIZE>,
    ) -> Result<(), DI::Error> {
        self.send_command_async(0x2E).await?;
        self.set_vertical_scroll_area_inner_async(area).await
    }

    /// Async version of [`Self::configure_diagonal_scroll`].
    pub async fn configure_diagonal_scroll_async(
        &mut self,
        direction: ScrollDirection,
        pages: PageRange<SIZE>,
        interval: ScrollFrameInterval,
        area: VerticalScrollArea<SIZE>,
        vertical_offset: DisplayLine<SIZE>,
    ) -> Result<(), DI::Error> {
        if !area.supports_offset(vertical_offset) {
            return Err(Error::InvalidArgument(
                InvalidArgument::VerticalScrollOffsetOutOfRange,
            ));
        }

        self.send_command_async(0x2E).await?;
        self.set_vertical_scroll_area_inner_async(area).await?;
        self.send_commands_async(&[
            diagonal_scroll_command(direction),
            0x00,
            pages.start().as_u8(),
            scroll_interval_bits(interval),
            pages.end().as_u8(),
            vertical_offset.as_u8(),
        ])
        .await
    }

    /// Async version of [`Self::start_scroll`].
    pub async fn start_scroll_async(
        mut self,
    ) -> StateChangeResult<Ssd1306<DI, SIZE, MODE, ScrollActive>, Self, DI::Error> {
        if let Err(error) = self.send_command_async(0x2F).await {
            return Err(StateChangeError::new(self, error));
        }

        Ok(Ssd1306 {
            i2c: self.i2c,
            address: self.address,
            mode: self.mode,
            config: self.config,
            _size: PhantomData,
            _scroll: PhantomData,
        })
    }
}

#[cfg(feature = "async")]
impl<DI, SIZE, MODE> Ssd1306<DI, SIZE, MODE, ScrollActive>
where
    DI: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
    SIZE: DisplaySize,
{
    /// Async version of [`Self::stop_scroll`].
    pub async fn stop_scroll_async(
        mut self,
    ) -> StateChangeResult<Ssd1306<DI, SIZE, MODE, ScrollRestoreRequired>, Self, DI::Error> {
        if let Err(error) = self.send_command_async(0x2E).await {
            return Err(StateChangeError::new(self, error));
        }

        Ok(Ssd1306 {
            i2c: self.i2c,
            address: self.address,
            mode: self.mode,
            config: self.config,
            _size: PhantomData,
            _scroll: PhantomData,
        })
    }
}

fn send_frame<I2C>(i2c: &mut I2C, address: u8, control: u8, bytes: &[u8]) -> Result<(), I2C::Error>
where
    I2C: I2c<SevenBitAddress>,
{
    if bytes.is_empty() {
        return Ok(());
    }

    let mut packet = [0u8; CHUNK_SIZE + 1];
    packet[0] = control;

    for chunk in bytes.chunks(CHUNK_SIZE) {
        let len = chunk.len();
        packet[1..1 + len].copy_from_slice(chunk);
        i2c.write(address, &packet[..1 + len]).map_err(Error::Bus)?;
    }

    Ok(())
}

#[cfg(feature = "async")]
async fn send_frame_async<I2C>(
    i2c: &mut I2C,
    address: u8,
    control: u8,
    bytes: &[u8],
) -> Result<(), I2C::Error>
where
    I2C: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
{
    if bytes.is_empty() {
        return Ok(());
    }

    let mut packet = [0u8; CHUNK_SIZE + 1];
    packet[0] = control;

    for chunk in bytes.chunks(CHUNK_SIZE) {
        let len = chunk.len();
        packet[1..1 + len].copy_from_slice(chunk);
        i2c.write(address, &packet[..1 + len])
            .await
            .map_err(Error::Bus)?;
    }

    Ok(())
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

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    #[cfg(feature = "async")]
    use core::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    use embedded_hal::i2c::{ErrorKind, ErrorType, Operation};
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
                VerticalScrollArea::new(RowCount::new(8).unwrap(), RowCount::new(8).unwrap())
                    .unwrap(),
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
        let mut display =
            Ssd1306::new(MockI2c::fail_after(0), DisplaySize128x64, Rotation::Rotate0);

        assert_eq!(
            display.set_rotation(Rotation::Rotate0),
            Err(Error::Bus(MockI2cError))
        );
    }

    #[test]
    fn set_orientation_failure_preserves_previous_orientation() {
        let mut display =
            Ssd1306::new(MockI2c::fail_after(0), DisplaySize128x64, Rotation::Rotate0);

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
        let mut display =
            Ssd1306::new(MockI2c::fail_after(2), DisplaySize128x64, Rotation::Rotate0);

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
        let mut display =
            Ssd1306::new(MockI2c::fail_after(0), DisplaySize128x64, Rotation::Rotate0);

        assert_eq!(display.orientation(), Orientation::ROTATE_0);

        let result = block_on(display.set_orientation_async(Orientation::ROTATE_180));

        assert_eq!(result, Err(Error::Bus(MockI2cError)));
        assert_eq!(display.orientation(), Orientation::ROTATE_0);
        assert_eq!(display.rotation(), Some(Rotation::Rotate0));
    }

    #[cfg(feature = "async")]
    #[test]
    fn init_with_config_async_failure_preserves_previous_config() {
        let mut display =
            Ssd1306::new(MockI2c::fail_after(2), DisplaySize128x64, Rotation::Rotate0);

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
}
