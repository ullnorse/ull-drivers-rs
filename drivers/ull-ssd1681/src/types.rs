/// Driver result type.
pub type Result<T, E> = core::result::Result<T, Error<E>>;

/// Panel width in pixels (GDEY0154D67 datasheet p.5).
pub const WIDTH: usize = 200;

/// Panel height in pixels (GDEY0154D67 datasheet p.5).
pub const HEIGHT: usize = 200;

/// Bytes per frame row: 8 pixels per byte.
pub const BYTES_PER_ROW: usize = WIDTH / 8;

/// Size of one frame in bytes: 200 x 200 pixels, 1 bit each.
///
/// The layout is the SSD1681 RAM layout with the data entry mode used by this
/// driver: rows top to bottom, 25 bytes per row left to right, the leftmost
/// pixel of a byte in bit 7. A set bit is white, a clear bit is black
/// (SSD1681 datasheet p.26, command `0x24`).
pub const FRAME_LEN: usize = BYTES_PER_ROW * HEIGHT;

/// Typical full update time at 25 C: 2 s (GDEY0154D67 datasheet p.21; p.27
/// lists 3 s as the image update time).
pub const FULL_REFRESH_TYPICAL_MS: u32 = 2_000;

/// Typical partial update time at 25 C: 0.26 s (GDEY0154D67 datasheet p.21).
pub const PARTIAL_REFRESH_TYPICAL_MS: u32 = 260;

/// The panel datasheet recommends a full refresh after 5 consecutive partial
/// refreshes to limit afterimages (GDEY0154D67 datasheet p.22).
pub const PARTIALS_BEFORE_FULL: u8 = 5;

/// BUSY timeouts, in milliseconds.
///
/// The datasheets give typical times only. The defaults are several times the
/// typical value because waveforms are slower at low temperature.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Config {
    /// Timeout for commands that raise BUSY briefly: software reset, RAM
    /// auto-fill, LUT load, and the wait after a hardware reset.
    pub command_timeout_ms: u32,
    /// Timeout for a full refresh (typical 2 s).
    pub full_refresh_timeout_ms: u32,
    /// Timeout for a partial refresh (typical 0.26 s).
    pub partial_refresh_timeout_ms: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            command_timeout_ms: 1_000,
            full_refresh_timeout_ms: 3 * FULL_REFRESH_TYPICAL_MS,
            partial_refresh_timeout_ms: 1_500,
        }
    }
}

/// Deep sleep mode argument of command `0x10`.
///
/// Mode 1 keeps the display RAM but it cannot be accessed, mode 2 does not
/// keep it (SSD1681 datasheet p.36, Table 10-1; panel datasheet p.21, "Ram
/// data retain" / "Ram data not retain"). Codes: SSD1681 p.23, panel p.11.
/// Leaving deep sleep requires a hardware reset (SSD1681 p.23, panel p.11).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DeepSleepMode {
    /// `0b01`: deep sleep mode 1, RAM data retained but not accessible.
    Mode1,
    /// `0b11`: deep sleep mode 2, RAM data not retained.
    Mode2,
}

impl DeepSleepMode {
    /// Returns the command parameter byte.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Mode1 => 0b01,
            Self::Mode2 => 0b11,
        }
    }
}

/// Pin that failed.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Pin {
    /// Data/command select pin.
    DataCommand,
    /// Reset pin.
    Reset,
    /// BUSY input pin.
    Busy,
}

/// Errors returned by the driver.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error<BusError> {
    /// SPI transport error.
    Bus(BusError),
    /// A GPIO pin returned an error.
    Pin(Pin),
    /// BUSY stayed high past the configured timeout.
    BusyTimeout,
}

impl<BusError> core::fmt::Display for Error<BusError>
where
    BusError: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bus(error) => write!(f, "e-paper bus error: {error:?}"),
            Self::Pin(pin) => write!(f, "e-paper pin error: {pin:?}"),
            Self::BusyTimeout => f.write_str("e-paper BUSY timeout"),
        }
    }
}

impl<BusError> core::error::Error for Error<BusError> where BusError: core::fmt::Debug {}
