use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;

use crate::types::{DeepSleepMode, Error, FRAME_LEN, Pin, Result};

use super::{
    AUTO_FILL_WHITE, BORDER_FULL_DATA, BORDER_PARTIAL_DATA, CMD_AUTO_FILL_BW, CMD_AUTO_FILL_RED,
    CMD_BORDER, CMD_DEEP_SLEEP, CMD_DISPLAY_UPDATE_2, CMD_MASTER_ACTIVATION, CMD_RAM_X_COUNTER,
    CMD_RAM_Y_COUNTER, CMD_SW_RESET, CMD_WRITE_BW_RAM, CMD_WRITE_RED_RAM, CONFIG_SEQUENCE,
    RAM_X_COUNTER_START, RAM_Y_COUNTER_START, RESET_MS, SW_RESET_WAIT_MS, Ssd1681, UPDATE_FULL,
    UPDATE_LOAD_LUT, UPDATE_PARTIAL,
};

impl<SPI, DC, RST, BUSY, DELAY> Ssd1681<SPI, DC, RST, BUSY, DELAY>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    DELAY: DelayNs,
{
    /// Pulses the hardware reset pin: low, wait, high, wait (panel datasheet
    /// p.31 step 2). This is also the only way out of deep sleep
    /// (SSD1681 p.23).
    pub fn hardware_reset(&mut self) -> Result<(), SPI::Error> {
        self.rst.set_low().map_err(|_| Error::Pin(Pin::Reset))?;
        self.delay.delay_ms(RESET_MS);
        self.rst.set_high().map_err(|_| Error::Pin(Pin::Reset))?;
        self.delay.delay_ms(RESET_MS);
        Ok(())
    }

    /// Resets the controller and sends the panel's init sequence, including
    /// the white fill of both RAMs (panel datasheet p.31 steps 2 to 4).
    ///
    /// The RAM fill overwrites the "previous image" RAM. After a deep sleep
    /// that kept RAM, use [`wake`](Self::wake) instead.
    pub fn init(&mut self) -> Result<(), SPI::Error> {
        self.reset_and_configure()?;
        self.write_command(CMD_AUTO_FILL_BW, &AUTO_FILL_WHITE)?;
        self.wait_until_idle(self.config.command_timeout_ms)?;
        self.write_command(CMD_AUTO_FILL_RED, &AUTO_FILL_WHITE)?;
        self.wait_until_idle(self.config.command_timeout_ms)?;
        self.load_lut()
    }

    /// Resets the controller and sends the init sequence without touching RAM.
    ///
    /// Use after [`deep_sleep`](Self::deep_sleep) with
    /// [`DeepSleepMode::Mode1`] to keep the previous image. Whether a hardware
    /// reset itself leaves RAM intact is not stated in either datasheet.
    pub fn wake(&mut self) -> Result<(), SPI::Error> {
        self.reset_and_configure()?;
        self.load_lut()
    }

    /// Sets both RAMs to white and runs a full refresh.
    pub fn clear(&mut self) -> Result<(), SPI::Error> {
        self.write_command(CMD_AUTO_FILL_BW, &AUTO_FILL_WHITE)?;
        self.wait_until_idle(self.config.command_timeout_ms)?;
        self.write_command(CMD_AUTO_FILL_RED, &AUTO_FILL_WHITE)?;
        self.wait_until_idle(self.config.command_timeout_ms)?;
        self.full_refresh()
    }

    /// Writes a frame to the black/white RAM (`0x24`), the new image.
    ///
    /// Byte `y * 25 + x / 8` holds pixel `(x, y)` in bit `7 - x % 8`. A set
    /// bit is white, a clear bit is black (SSD1681 datasheet p.26, panel
    /// datasheet p.14).
    pub fn write_frame(&mut self, frame: &[u8; FRAME_LEN]) -> Result<(), SPI::Error> {
        self.write_ram(CMD_WRITE_BW_RAM, frame)
    }

    /// Writes a frame to the `0x26` RAM, the "previous image" that a partial
    /// refresh compares against.
    pub fn write_previous_frame(&mut self, frame: &[u8; FRAME_LEN]) -> Result<(), SPI::Error> {
        self.write_ram(CMD_WRITE_RED_RAM, frame)
    }

    /// Refreshes the panel with the full waveform (about 2 s, flickers).
    pub fn full_refresh(&mut self) -> Result<(), SPI::Error> {
        self.write_command(CMD_BORDER, &BORDER_FULL_DATA)?;
        self.write_command(CMD_DISPLAY_UPDATE_2, &UPDATE_FULL)?;
        self.write_command(CMD_MASTER_ACTIVATION, &[])?;
        self.wait_until_idle(self.config.full_refresh_timeout_ms)
    }

    /// Refreshes the panel with the partial waveform (about 0.26 s, no
    /// flicker).
    ///
    /// The new image must be in `0x24` and the image on the panel must be in
    /// `0x26`. After the refresh, write the new image to `0x26` as well
    /// ([`write_previous_frame`](Self::write_previous_frame)), or use
    /// [`display_partial`](Self::display_partial).
    pub fn partial_refresh(&mut self) -> Result<(), SPI::Error> {
        self.write_command(CMD_BORDER, &BORDER_PARTIAL_DATA)?;
        self.write_command(CMD_DISPLAY_UPDATE_2, &UPDATE_PARTIAL)?;
        self.write_command(CMD_MASTER_ACTIVATION, &[])?;
        self.wait_until_idle(self.config.partial_refresh_timeout_ms)
    }

    /// Writes `frame` to both RAMs and runs a full refresh. Afterwards a
    /// partial refresh has a valid previous image.
    pub fn display_full(&mut self, frame: &[u8; FRAME_LEN]) -> Result<(), SPI::Error> {
        self.write_frame(frame)?;
        self.write_previous_frame(frame)?;
        self.full_refresh()
    }

    /// Writes `frame` as the new image, runs a partial refresh, then copies
    /// `frame` to the previous-image RAM for the next partial refresh.
    ///
    /// The panel datasheet recommends a full refresh after 5 consecutive
    /// partial refreshes (p.22, [`PARTIALS_BEFORE_FULL`](crate::PARTIALS_BEFORE_FULL)).
    pub fn display_partial(&mut self, frame: &[u8; FRAME_LEN]) -> Result<(), SPI::Error> {
        self.write_frame(frame)?;
        self.partial_refresh()?;
        self.write_previous_frame(frame)
    }

    /// Enters deep sleep (command `0x10`). BUSY stays high, so the driver does
    /// not wait. Leave with [`wake`](Self::wake) or [`init`](Self::init),
    /// which start with a hardware reset.
    ///
    /// Mode 1 retains RAM but RAM cannot be accessed; mode 2 does not retain
    /// RAM (SSD1681 datasheet p.36, panel datasheet p.21).
    pub fn deep_sleep(&mut self, mode: DeepSleepMode) -> Result<(), SPI::Error> {
        self.write_command(CMD_DEEP_SLEEP, &[mode.as_u8()])
    }

    /// Sends a raw command with its parameter or RAM data bytes.
    ///
    /// The command byte goes out with D/C low, the data (if any) with D/C
    /// high, as two SPI writes.
    pub fn write_command(&mut self, command: u8, data: &[u8]) -> Result<(), SPI::Error> {
        self.dc
            .set_low()
            .map_err(|_| Error::Pin(Pin::DataCommand))?;
        self.spi.write(&[command]).map_err(Error::Bus)?;
        if !data.is_empty() {
            self.dc
                .set_high()
                .map_err(|_| Error::Pin(Pin::DataCommand))?;
            self.spi.write(data).map_err(Error::Bus)?;
        }
        Ok(())
    }

    /// Waits until BUSY is low, polling every millisecond. Returns
    /// [`Error::BusyTimeout`] after `timeout_ms`.
    pub fn wait_until_idle(&mut self, timeout_ms: u32) -> Result<(), SPI::Error> {
        let mut waited_ms = 0;
        while self.busy.is_high().map_err(|_| Error::Pin(Pin::Busy))? {
            if waited_ms >= timeout_ms {
                return Err(Error::BusyTimeout);
            }
            self.delay.delay_ms(1);
            waited_ms += 1;
        }
        Ok(())
    }

    fn reset_and_configure(&mut self) -> Result<(), SPI::Error> {
        self.hardware_reset()?;
        self.write_command(CMD_SW_RESET, &[])?;
        self.delay.delay_ms(SW_RESET_WAIT_MS);
        self.wait_until_idle(self.config.command_timeout_ms)?;
        for (command, data) in CONFIG_SEQUENCE {
            self.write_command(command, data)?;
        }
        Ok(())
    }

    fn load_lut(&mut self) -> Result<(), SPI::Error> {
        self.write_command(CMD_DISPLAY_UPDATE_2, &UPDATE_LOAD_LUT)?;
        self.write_command(CMD_MASTER_ACTIVATION, &[])?;
        self.wait_until_idle(self.config.command_timeout_ms)
    }

    fn write_ram(&mut self, command: u8, frame: &[u8; FRAME_LEN]) -> Result<(), SPI::Error> {
        self.write_command(CMD_RAM_X_COUNTER, &RAM_X_COUNTER_START)?;
        self.write_command(CMD_RAM_Y_COUNTER, &RAM_Y_COUNTER_START)?;
        self.write_command(command, frame)
    }
}
