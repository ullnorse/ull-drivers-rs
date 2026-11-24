use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::spi::SpiDevice;

use crate::types::{DeepSleepMode, Error, FRAME_LEN, Pin, Result};

use super::{
    AUTO_FILL_WHITE, BORDER_FULL_DATA, BORDER_PARTIAL_DATA, CMD_AUTO_FILL_BW, CMD_AUTO_FILL_RED,
    CMD_BORDER, CMD_DEEP_SLEEP, CMD_DISPLAY_UPDATE_2, CMD_MASTER_ACTIVATION, CMD_RAM_X_COUNTER,
    CMD_RAM_Y_COUNTER, CMD_SW_RESET, CMD_WRITE_BW_RAM, CMD_WRITE_RED_RAM, CONFIG_SEQUENCE,
    RAM_X_COUNTER_START, RAM_Y_COUNTER_START, RESET_MS, SW_RESET_WAIT_MS, Ssd1681, UPDATE_FULL,
    UPDATE_LOAD_LUT, UPDATE_PARTIAL,
};

#[cfg(feature = "async")]
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
    pub async fn hardware_reset_async(&mut self) -> Result<(), SPI::Error> {
        self.rst.set_low().map_err(|_| Error::Pin(Pin::Reset))?;
        self.delay.delay_ms(RESET_MS).await;
        self.rst.set_high().map_err(|_| Error::Pin(Pin::Reset))?;
        self.delay.delay_ms(RESET_MS).await;
        Ok(())
    }

    /// Resets the controller and sends the panel's init sequence, including
    /// the white fill of both RAMs (panel datasheet p.31 steps 2 to 4).
    ///
    /// The RAM fill overwrites the "previous image" RAM. After a deep sleep
    /// that kept RAM, use [`wake_async`](Self::wake_async) instead.
    pub async fn init_async(&mut self) -> Result<(), SPI::Error> {
        self.reset_and_configure_async().await?;
        self.write_command_async(CMD_AUTO_FILL_BW, &AUTO_FILL_WHITE)
            .await?;
        self.wait_until_idle_async(self.config.command_timeout_ms)
            .await?;
        self.write_command_async(CMD_AUTO_FILL_RED, &AUTO_FILL_WHITE)
            .await?;
        self.wait_until_idle_async(self.config.command_timeout_ms)
            .await?;
        self.load_lut_async().await
    }

    /// Resets the controller and sends the init sequence without touching RAM.
    ///
    /// Use after [`deep_sleep_async`](Self::deep_sleep_async) with
    /// [`DeepSleepMode::Mode1`] to keep the previous image. Whether a hardware
    /// reset itself leaves RAM intact is not stated in either datasheet.
    pub async fn wake_async(&mut self) -> Result<(), SPI::Error> {
        self.reset_and_configure_async().await?;
        self.load_lut_async().await
    }

    /// Sets both RAMs to white and runs a full refresh.
    pub async fn clear_async(&mut self) -> Result<(), SPI::Error> {
        self.write_command_async(CMD_AUTO_FILL_BW, &AUTO_FILL_WHITE)
            .await?;
        self.wait_until_idle_async(self.config.command_timeout_ms)
            .await?;
        self.write_command_async(CMD_AUTO_FILL_RED, &AUTO_FILL_WHITE)
            .await?;
        self.wait_until_idle_async(self.config.command_timeout_ms)
            .await?;
        self.full_refresh_async().await
    }

    /// Writes a frame to the black/white RAM (`0x24`), the new image.
    ///
    /// Byte `y * 25 + x / 8` holds pixel `(x, y)` in bit `7 - x % 8`. A set
    /// bit is white, a clear bit is black (SSD1681 datasheet p.26, panel
    /// datasheet p.14).
    pub async fn write_frame_async(&mut self, frame: &[u8; FRAME_LEN]) -> Result<(), SPI::Error> {
        self.write_ram_async(CMD_WRITE_BW_RAM, frame).await
    }

    /// Writes a frame to the `0x26` RAM, the "previous image" that a partial
    /// refresh compares against.
    pub async fn write_previous_frame_async(
        &mut self,
        frame: &[u8; FRAME_LEN],
    ) -> Result<(), SPI::Error> {
        self.write_ram_async(CMD_WRITE_RED_RAM, frame).await
    }

    /// Refreshes the panel with the full waveform (about 2 s, flickers).
    pub async fn full_refresh_async(&mut self) -> Result<(), SPI::Error> {
        self.write_command_async(CMD_BORDER, &BORDER_FULL_DATA)
            .await?;
        self.write_command_async(CMD_DISPLAY_UPDATE_2, &UPDATE_FULL)
            .await?;
        self.write_command_async(CMD_MASTER_ACTIVATION, &[]).await?;
        self.wait_until_idle_async(self.config.full_refresh_timeout_ms)
            .await
    }

    /// Refreshes the panel with the partial waveform (about 0.26 s, no
    /// flicker).
    ///
    /// The new image must be in `0x24` and the image on the panel must be in
    /// `0x26`. After the refresh, write the new image to `0x26` as well
    /// ([`write_previous_frame_async`](Self::write_previous_frame_async)), or use
    /// [`display_partial_async`](Self::display_partial_async).
    pub async fn partial_refresh_async(&mut self) -> Result<(), SPI::Error> {
        self.write_command_async(CMD_BORDER, &BORDER_PARTIAL_DATA)
            .await?;
        self.write_command_async(CMD_DISPLAY_UPDATE_2, &UPDATE_PARTIAL)
            .await?;
        self.write_command_async(CMD_MASTER_ACTIVATION, &[]).await?;
        self.wait_until_idle_async(self.config.partial_refresh_timeout_ms)
            .await
    }

    /// Writes `frame` to both RAMs and runs a full refresh. Afterwards a
    /// partial refresh has a valid previous image.
    pub async fn display_full_async(&mut self, frame: &[u8; FRAME_LEN]) -> Result<(), SPI::Error> {
        self.write_frame_async(frame).await?;
        self.write_previous_frame_async(frame).await?;
        self.full_refresh_async().await
    }

    /// Writes `frame` as the new image, runs a partial refresh, then copies
    /// `frame` to the previous-image RAM for the next partial refresh.
    ///
    /// The panel datasheet recommends a full refresh after 5 consecutive
    /// partial refreshes (p.22, [`PARTIALS_BEFORE_FULL`](crate::PARTIALS_BEFORE_FULL)).
    pub async fn display_partial_async(
        &mut self,
        frame: &[u8; FRAME_LEN],
    ) -> Result<(), SPI::Error> {
        self.write_frame_async(frame).await?;
        self.partial_refresh_async().await?;
        self.write_previous_frame_async(frame).await
    }

    /// Enters deep sleep (command `0x10`). BUSY stays high, so the driver does
    /// not wait. Leave with [`wake_async`](Self::wake_async) or [`init_async`](Self::init_async),
    /// which start with a hardware reset.
    ///
    /// Mode 1 retains RAM but RAM cannot be accessed; mode 2 does not retain
    /// RAM (SSD1681 datasheet p.36, panel datasheet p.21).
    pub async fn deep_sleep_async(&mut self, mode: DeepSleepMode) -> Result<(), SPI::Error> {
        self.write_command_async(CMD_DEEP_SLEEP, &[mode.as_u8()])
            .await
    }

    /// Sends a raw command with its parameter or RAM data bytes.
    ///
    /// The command byte goes out with D/C low, the data (if any) with D/C
    /// high, as two SPI writes.
    pub async fn write_command_async(
        &mut self,
        command: u8,
        data: &[u8],
    ) -> Result<(), SPI::Error> {
        self.dc
            .set_low()
            .map_err(|_| Error::Pin(Pin::DataCommand))?;
        self.spi.write(&[command]).await.map_err(Error::Bus)?;
        if !data.is_empty() {
            self.dc
                .set_high()
                .map_err(|_| Error::Pin(Pin::DataCommand))?;
            self.spi.write(data).await.map_err(Error::Bus)?;
        }
        Ok(())
    }

    /// Waits until BUSY is low, polling every millisecond. Returns
    /// [`Error::BusyTimeout`] after `timeout_ms`.
    pub async fn wait_until_idle_async(&mut self, timeout_ms: u32) -> Result<(), SPI::Error> {
        let mut waited_ms = 0;
        while self.busy.is_high().map_err(|_| Error::Pin(Pin::Busy))? {
            if waited_ms >= timeout_ms {
                return Err(Error::BusyTimeout);
            }
            self.delay.delay_ms(1).await;
            waited_ms += 1;
        }
        Ok(())
    }

    async fn reset_and_configure_async(&mut self) -> Result<(), SPI::Error> {
        self.hardware_reset_async().await?;
        self.write_command_async(CMD_SW_RESET, &[]).await?;
        self.delay.delay_ms(SW_RESET_WAIT_MS).await;
        self.wait_until_idle_async(self.config.command_timeout_ms)
            .await?;
        for (command, data) in CONFIG_SEQUENCE {
            self.write_command_async(command, data).await?;
        }
        Ok(())
    }

    async fn load_lut_async(&mut self) -> Result<(), SPI::Error> {
        self.write_command_async(CMD_DISPLAY_UPDATE_2, &UPDATE_LOAD_LUT)
            .await?;
        self.write_command_async(CMD_MASTER_ACTIVATION, &[]).await?;
        self.wait_until_idle_async(self.config.command_timeout_ms)
            .await
    }

    async fn write_ram_async(
        &mut self,
        command: u8,
        frame: &[u8; FRAME_LEN],
    ) -> Result<(), SPI::Error> {
        self.write_command_async(CMD_RAM_X_COUNTER, &RAM_X_COUNTER_START)
            .await?;
        self.write_command_async(CMD_RAM_Y_COUNTER, &RAM_Y_COUNTER_START)
            .await?;
        self.write_command_async(command, frame).await
    }
}
