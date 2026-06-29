use embedded_hal::digital::OutputPin;

use crate::size::DisplaySize;
use crate::types::{
    Config, DisplayLine, DisplayOffset, Error, InitError, InvalidArgument, Orientation, PageRange,
    Result, Rotation, ScrollDirection, ScrollFrameInterval, SegmentRemap, VerticalScrollArea,
};

use super::{
    BufferedGraphicsMode, CHUNK_SIZE, CONTROL_COMMAND, CONTROL_DATA, ComScanDirection, DrawArea,
    DriverStateChangeResult, RawMode, ScrollActive, ScrollInactive, ScrollRestoreRequired, Ssd1306,
    StateChangeError, diagonal_scroll_setup, frame_packet, horizontal_scroll_setup, init_prefix,
    init_suffix, map_init_error, orientation_commands, vertical_scroll_area_setup,
};

#[cfg(feature = "async")]
impl<DI, SIZE, MODE> Ssd1306<DI, SIZE, MODE, ScrollInactive>
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
        self.send_commands_async(&init_prefix::<SIZE>(config))
            .await?;
        self.apply_orientation_async(config.orientation).await?;
        self.send_commands_async(&init_suffix::<SIZE>(config))
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
            .map_err(map_init_error)
    }

    /// Async version of [`Self::configure_horizontal_scroll`].
    pub async fn configure_horizontal_scroll_async(
        &mut self,
        direction: ScrollDirection,
        pages: PageRange<SIZE>,
        interval: ScrollFrameInterval,
    ) -> Result<(), DI::Error> {
        self.send_command_async(0x2E).await?;
        self.send_commands_async(&horizontal_scroll_setup(direction, pages, interval))
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
        self.send_commands_async(&diagonal_scroll_setup(
            direction,
            pages,
            interval,
            vertical_offset,
        ))
        .await
    }

    /// Async version of [`Self::start_scroll`].
    pub async fn start_scroll_async(
        mut self,
    ) -> DriverStateChangeResult<DI, SIZE, MODE, ScrollInactive, ScrollActive, DI::Error> {
        if let Err(error) = self.send_command_async(0x2F).await {
            return Err(StateChangeError::new(self, error));
        }

        Ok(self.into_scroll_state())
    }
}

#[cfg(feature = "async")]
impl<DI, SIZE> Ssd1306<DI, SIZE, RawMode, ScrollInactive>
where
    DI: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
    SIZE: DisplaySize,
{
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

#[cfg(feature = "async")]
impl<DI, SIZE, SCROLL> Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>, SCROLL>
where
    DI: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
    SIZE: DisplaySize,
{
    async fn flush_buffer_area_async(&mut self, area: DrawArea) -> Result<(), DI::Error> {
        self.set_horizontal_addressing_mode_async().await?;
        self.set_draw_area_async(area).await?;

        let address = self.address;
        let i2c = &mut self.i2c;
        let buffer = self.mode.buffer.as_ref();

        if area.is_full::<SIZE>() {
            return send_frame_async(i2c, address, CONTROL_DATA, buffer).await;
        }

        let page_width = usize::from(SIZE::WIDTH);
        let (start_column, end_column) = area.column_range();

        for page in area.start_page..=area.end_page {
            let page_offset = usize::from(page) * page_width;
            send_frame_async(
                i2c,
                address,
                CONTROL_DATA,
                &buffer[page_offset + start_column..page_offset + end_column],
            )
            .await?;
        }

        Ok(())
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
        self.flush_buffer_area_async(DrawArea::full::<SIZE>()).await
    }

    /// Async version of [`Self::flush_area`].
    pub async fn flush_area_async(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), DI::Error> {
        let Some(area) = DrawArea::clipped::<SIZE>(x, y, width, height) else {
            return Ok(());
        };

        self.flush_buffer_area_async(area).await
    }
}

#[cfg(feature = "async")]
impl<DI, SIZE> Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>, ScrollRestoreRequired>
where
    DI: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
    SIZE: DisplaySize,
{
    /// Async version of [`Self::restore_display`].
    pub async fn restore_display_async(
        mut self,
    ) -> DriverStateChangeResult<
        DI,
        SIZE,
        BufferedGraphicsMode<SIZE>,
        ScrollRestoreRequired,
        ScrollInactive,
        DI::Error,
    > {
        if let Err(error) = self.flush_buffer_area_async(DrawArea::full::<SIZE>()).await {
            return Err(StateChangeError::new(self, error));
        }

        Ok(self.into_scroll_state())
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
    ) -> DriverStateChangeResult<DI, SIZE, MODE, ScrollActive, ScrollRestoreRequired, DI::Error>
    {
        if let Err(error) = self.send_command_async(0x2E).await {
            return Err(StateChangeError::new(self, error));
        }

        Ok(self.into_scroll_state())
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
        self.send_commands_async(&orientation_commands(orientation))
            .await
    }

    async fn set_horizontal_addressing_mode_async(&mut self) -> Result<(), DI::Error> {
        self.send_commands_async(&[0x20, 0x00]).await
    }

    async fn set_draw_area_async(&mut self, area: DrawArea) -> Result<(), DI::Error> {
        self.send_commands_async(&area.commands()).await
    }

    async fn set_vertical_scroll_area_inner_async(
        &mut self,
        area: VerticalScrollArea<SIZE>,
    ) -> Result<(), DI::Error> {
        self.send_commands_async(&vertical_scroll_area_setup(area))
            .await
    }
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

    for chunk in bytes.chunks(CHUNK_SIZE) {
        let frame = frame_packet(&mut packet, control, chunk);
        i2c.write(address, frame).await.map_err(Error::Bus)?;
    }

    Ok(())
}
