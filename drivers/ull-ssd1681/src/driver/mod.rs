mod blocking;

#[cfg(feature = "async")]
mod r#async;

use crate::types::Config;

// Command bytes. SSD1681 datasheet pp.20-33 command table; the panel datasheet
// (GDEY0154D67) repeats the table on pp.9-20 with the same opcodes. Page
// numbers are the printed page numbers of each PDF.

/// Driver output control (SSD1681 p.20, panel p.9).
const CMD_DRIVER_OUTPUT: u8 = 0x01;
/// Deep sleep mode (SSD1681 p.23, panel p.11).
const CMD_DEEP_SLEEP: u8 = 0x10;
/// Data entry mode setting (SSD1681 p.23, panel p.13).
const CMD_DATA_ENTRY: u8 = 0x11;
/// Software reset (SSD1681 p.23, panel p.12).
const CMD_SW_RESET: u8 = 0x12;
/// Temperature sensor control (SSD1681 p.24, panel p.12).
const CMD_TEMP_SENSOR: u8 = 0x18;
/// Master activation (SSD1681 p.25, panel p.13).
const CMD_MASTER_ACTIVATION: u8 = 0x20;
/// Display update control 2 (SSD1681 p.26, panel p.14).
const CMD_DISPLAY_UPDATE_2: u8 = 0x22;
/// Write RAM (black/white), RAM 0x24 (SSD1681 p.26, panel p.14).
const CMD_WRITE_BW_RAM: u8 = 0x24;
/// Write RAM (red), RAM 0x26: the "previous image" (SSD1681 p.27, panel p.15).
const CMD_WRITE_RED_RAM: u8 = 0x26;
/// Border waveform control (SSD1681 p.31, panel p.19).
const CMD_BORDER: u8 = 0x3C;
/// Set RAM X address start/end (SSD1681 p.31, panel p.19).
const CMD_RAM_X_RANGE: u8 = 0x44;
/// Set RAM Y address start/end (SSD1681 p.31, panel p.19).
const CMD_RAM_Y_RANGE: u8 = 0x45;
/// Auto write RED RAM for regular pattern (SSD1681 p.32, panel p.20).
const CMD_AUTO_FILL_RED: u8 = 0x46;
/// Auto write B/W RAM for regular pattern (SSD1681 p.32, panel p.20).
const CMD_AUTO_FILL_BW: u8 = 0x47;
/// Set RAM X address counter (SSD1681 p.33, panel p.20).
const CMD_RAM_X_COUNTER: u8 = 0x4E;
/// Set RAM Y address counter (SSD1681 p.33, panel p.20).
const CMD_RAM_Y_COUNTER: u8 = 0x4F;

/// Gate setting: 200 MUX (`0xC7` + 1 lines), scan direction POR
/// (SSD1681 p.20, panel p.9; the panel flow on p.31 sets this command).
const DRIVER_OUTPUT_DATA: [u8; 3] = [0xC7, 0x00, 0x00];
/// Data entry mode: X and Y increment, address counter moves along X
/// (`0b011`, the power-on value; SSD1681 p.23, panel p.13). Rows are written
/// top to bottom, left to right.
const DATA_ENTRY_DATA: [u8; 1] = [0x03];
/// X range in RAM bytes: `0x00..=0x18`, 25 bytes = 200 pixels (SSD1681 p.31).
const RAM_X_RANGE_DATA: [u8; 2] = [0x00, 0x18];
/// Y range in rows: `0x000..=0x0C7` (SSD1681 p.31): start low, start high,
/// end low, end high.
const RAM_Y_RANGE_DATA: [u8; 4] = [0x00, 0x00, 0xC7, 0x00];
/// Border waveform for full refresh: GS transition, follow LUT, LUT1 (white)
/// (SSD1681 p.31, panel p.19 bit fields).
const BORDER_FULL_DATA: [u8; 1] = [0x05];
/// Border waveform for partial refresh: VBD = VCOM, so the border does not
/// flash (panel p.19 bit fields).
const BORDER_PARTIAL_DATA: [u8; 1] = [0x80];
/// Internal temperature sensor (`0x80`, panel p.12). The power-on value
/// `0x48` selects an external sensor, which the board does not have.
const TEMP_SENSOR_INTERNAL: [u8; 1] = [0x80];
/// Auto fill pattern `0xF7`: first value 1 (white), step height and width
/// 200, so the whole RAM is filled with 1 (panel p.31 step 3, p.20).
const AUTO_FILL_WHITE: [u8; 1] = [0xF7];
/// Display update control 2 for the LUT load of the init flow: enable clock,
/// load temperature, load LUT with display mode 1, disable clock
/// (`0xB1`, SSD1681 p.26, panel p.14 table).
const UPDATE_LOAD_LUT: [u8; 1] = [0xB1];
/// Display update control 2 for a full refresh: enable clock and analog, load
/// temperature, load LUT and display with display mode 1, disable analog and
/// clock (`0xF7`, panel p.14 table).
const UPDATE_FULL: [u8; 1] = [0xF7];
/// Display update control 2 for a partial refresh: as [`UPDATE_FULL`] but
/// with display mode 2 (`0xFF`, panel p.14 table). Mode 2 is the OTP waveform
/// used for partial refresh.
const UPDATE_PARTIAL: [u8; 1] = [0xFF];
/// Cursor start: X counter 0, Y counter 0 (`0x4F` takes two bytes).
const RAM_X_COUNTER_START: [u8; 1] = [0x00];
const RAM_Y_COUNTER_START: [u8; 2] = [0x00, 0x00];

/// Hardware reset pulse width and recovery time. The panel flow only says
/// "wait 10 ms" (panel p.31); neither datasheet gives a reset pulse width.
const RESET_MS: u32 = 10;
/// Wait after software reset before BUSY is polled (panel p.31: "Wait 10 ms").
const SW_RESET_WAIT_MS: u32 = 10;

/// Commands sent after a software reset, in order (panel p.31 steps 3 and 4,
/// SSD1681 p.34 steps 3 and 4).
const CONFIG_SEQUENCE: [(u8, &[u8]); 6] = [
    (CMD_DRIVER_OUTPUT, &DRIVER_OUTPUT_DATA),
    (CMD_DATA_ENTRY, &DATA_ENTRY_DATA),
    (CMD_RAM_X_RANGE, &RAM_X_RANGE_DATA),
    (CMD_RAM_Y_RANGE, &RAM_Y_RANGE_DATA),
    (CMD_BORDER, &BORDER_FULL_DATA),
    (CMD_TEMP_SENSOR, &TEMP_SENSOR_INTERNAL),
];

/// SSD1681 e-paper controller on a GDEY0154D67 panel.
///
/// The driver owns the SPI device (chip select is handled by the
/// `SpiDevice`), the data/command and reset output pins, the BUSY input pin
/// and a delay provider.
#[derive(Debug)]
pub struct Ssd1681<SPI, DC, RST, BUSY, DELAY> {
    spi: SPI,
    dc: DC,
    rst: RST,
    busy: BUSY,
    delay: DELAY,
    config: Config,
}

impl<SPI, DC, RST, BUSY, DELAY> Ssd1681<SPI, DC, RST, BUSY, DELAY> {
    /// Creates a driver with the default [`Config`]. No bus traffic happens
    /// until [`init`](Self::init).
    #[must_use]
    pub fn new(spi: SPI, dc: DC, rst: RST, busy: BUSY, delay: DELAY) -> Self {
        Self::with_config(spi, dc, rst, busy, delay, Config::default())
    }

    /// Creates a driver with explicit BUSY timeouts.
    #[must_use]
    pub fn with_config(
        spi: SPI,
        dc: DC,
        rst: RST,
        busy: BUSY,
        delay: DELAY,
        config: Config,
    ) -> Self {
        Self {
            spi,
            dc,
            rst,
            busy,
            delay,
            config,
        }
    }

    /// Returns the BUSY timeouts in use.
    #[must_use]
    pub const fn config(&self) -> Config {
        self.config
    }

    /// Returns the owned peripherals.
    #[must_use]
    pub fn release(self) -> (SPI, DC, RST, BUSY, DELAY) {
        (self.spi, self.dc, self.rst, self.busy, self.delay)
    }
}
