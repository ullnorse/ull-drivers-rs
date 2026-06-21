use core::marker::PhantomData;

use crate::size::DisplaySize;

/// Driver result type.
pub type Result<T, E> = core::result::Result<T, Error<E>>;

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

/// Initialization options applied by [`crate::Ssd1306::init_with_config`].
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
