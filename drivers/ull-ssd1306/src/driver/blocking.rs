use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::i2c::{I2c, SevenBitAddress};

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

impl<DI, SIZE, MODE> Ssd1306<DI, SIZE, MODE, ScrollInactive>
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
        self.send_commands(&init_prefix::<SIZE>(config))?;
        self.apply_orientation(config.orientation)?;
        self.send_commands(&init_suffix::<SIZE>(config))?;
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
        self.init_with_config(config).map_err(map_init_error)
    }

    /// Configures a hardware horizontal scroll operation.
    pub fn configure_horizontal_scroll(
        &mut self,
        direction: ScrollDirection,
        pages: PageRange<SIZE>,
        interval: ScrollFrameInterval,
    ) -> Result<(), DI::Error> {
        self.send_command(0x2E)?;
        self.send_commands(&horizontal_scroll_setup(direction, pages, interval))
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
        self.send_commands(&diagonal_scroll_setup(
            direction,
            pages,
            interval,
            vertical_offset,
        ))
    }

    /// Activates the previously configured hardware scroll operation.
    pub fn start_scroll(
        mut self,
    ) -> DriverStateChangeResult<DI, SIZE, MODE, ScrollInactive, ScrollActive, DI::Error> {
        if let Err(error) = self.send_command(0x2F) {
            return Err(StateChangeError::new(self, error));
        }

        Ok(self.into_scroll_state())
    }
}

impl<DI, SIZE> Ssd1306<DI, SIZE, RawMode, ScrollInactive>
where
    DI: I2c<SevenBitAddress>,
    SIZE: DisplaySize,
{
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

impl<DI, SIZE, SCROLL> Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>, SCROLL>
where
    DI: I2c<SevenBitAddress>,
    SIZE: DisplaySize,
{
    fn flush_buffer_area(&mut self, area: DrawArea) -> Result<(), DI::Error> {
        self.set_horizontal_addressing_mode()?;
        self.set_draw_area(area)?;

        let address = self.address;
        let i2c = &mut self.i2c;
        let buffer = self.mode.buffer.as_ref();

        if area.is_full::<SIZE>() {
            return send_frame(i2c, address, CONTROL_DATA, buffer);
        }

        let page_width = usize::from(SIZE::WIDTH);
        let (start_column, end_column) = area.column_range();

        for page in area.start_page..=area.end_page {
            let page_offset = usize::from(page) * page_width;
            send_frame(
                i2c,
                address,
                CONTROL_DATA,
                &buffer[page_offset + start_column..page_offset + end_column],
            )?;
        }

        Ok(())
    }
}

impl<DI, SIZE> Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>, ScrollInactive>
where
    DI: I2c<SevenBitAddress>,
    SIZE: DisplaySize,
{
    /// Uploads the full framebuffer to display RAM.
    pub fn flush(&mut self) -> Result<(), DI::Error> {
        self.flush_buffer_area(DrawArea::full::<SIZE>())
    }

    /// Uploads a framebuffer sub-area to display RAM.
    ///
    /// SSD1306 RAM is page-oriented, so the vertical range is rounded out to the
    /// affected 8-pixel pages.
    pub fn flush_area(&mut self, x: u32, y: u32, width: u32, height: u32) -> Result<(), DI::Error> {
        let Some(area) = DrawArea::clipped::<SIZE>(x, y, width, height) else {
            return Ok(());
        };

        self.flush_buffer_area(area)
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
    ) -> DriverStateChangeResult<
        DI,
        SIZE,
        BufferedGraphicsMode<SIZE>,
        ScrollRestoreRequired,
        ScrollInactive,
        DI::Error,
    > {
        if let Err(error) = self.flush_buffer_area(DrawArea::full::<SIZE>()) {
            return Err(StateChangeError::new(self, error));
        }

        Ok(self.into_scroll_state())
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
    ) -> DriverStateChangeResult<DI, SIZE, MODE, ScrollActive, ScrollRestoreRequired, DI::Error>
    {
        if let Err(error) = self.send_command(0x2E) {
            return Err(StateChangeError::new(self, error));
        }

        Ok(self.into_scroll_state())
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
        self.send_commands(&orientation_commands(orientation))
    }

    fn set_horizontal_addressing_mode(&mut self) -> Result<(), DI::Error> {
        self.send_commands(&[0x20, 0x00])
    }

    fn set_draw_area(&mut self, area: DrawArea) -> Result<(), DI::Error> {
        self.send_commands(&area.commands())
    }

    fn set_vertical_scroll_area_inner(
        &mut self,
        area: VerticalScrollArea<SIZE>,
    ) -> Result<(), DI::Error> {
        self.send_commands(&vertical_scroll_area_setup(area))
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

    for chunk in bytes.chunks(CHUNK_SIZE) {
        let frame = frame_packet(&mut packet, control, chunk);
        i2c.write(address, frame).map_err(Error::Bus)?;
    }

    Ok(())
}
