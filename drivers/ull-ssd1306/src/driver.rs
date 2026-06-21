use core::marker::PhantomData;

#[cfg(feature = "graphics")]
use core::convert::Infallible;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::i2c::{I2c, SevenBitAddress};

use crate::size::{DisplaySize, private};
use crate::types::{
    Address, ComScanDirection, Config, DisplayLine, DisplayOffset, Error, InitError,
    InvalidArgument, Orientation, PageRange, PowerSource, Result, Rotation, ScrollDirection,
    ScrollFrameInterval, SegmentRemap, VerticalScrollArea,
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
    ) -> DriverStateChangeResult<
        DI,
        SIZE,
        BufferedGraphicsMode<SIZE>,
        ScrollRestoreRequired,
        ScrollInactive,
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
    ) -> DriverStateChangeResult<
        DI,
        SIZE,
        BufferedGraphicsMode<SIZE>,
        ScrollRestoreRequired,
        ScrollInactive,
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
    ) -> DriverStateChangeResult<DI, SIZE, MODE, ScrollInactive, ScrollActive, DI::Error> {
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
    ) -> DriverStateChangeResult<DI, SIZE, MODE, ScrollActive, ScrollRestoreRequired, DI::Error>
    {
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
    ) -> DriverStateChangeResult<DI, SIZE, MODE, ScrollInactive, ScrollActive, DI::Error> {
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
    ) -> DriverStateChangeResult<DI, SIZE, MODE, ScrollActive, ScrollRestoreRequired, DI::Error>
    {
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
