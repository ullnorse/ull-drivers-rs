use embedded_hal::{
    delay::DelayNs,
    i2c::{I2c, SevenBitAddress},
};

use crate::types::{
    Address, DataWord, Measurement, PeriodicRate, RawMeasurement, Repeatability, Result, Status,
    check_crc, map_fetch_error, parse_raw_measurement, parse_raw_temperature,
    temperature_celsius_from_raw, temperature_millicelsius_from_raw,
};

const GENERAL_CALL_ADDRESS: u8 = 0x00;

const CMD_FETCH_DATA: u16 = 0xE000;
const CMD_ART: u16 = 0x2B32;
const CMD_BREAK: u16 = 0x3093;
const CMD_SOFT_RESET: u16 = 0x30A2;
const CMD_HEATER_ENABLE: u16 = 0x306D;
const CMD_HEATER_DISABLE: u16 = 0x3066;
const CMD_READ_STATUS: u16 = 0xF32D;
const CMD_CLEAR_STATUS: u16 = 0x3041;

const COMMAND_DELAY_MS: u32 = 1;

/// Sensirion SHT3x-DIS embedded-hal 1.0 I2C driver.
///
/// Most applications should start with [`Self::measure`] for one-shot readings
/// or [`Self::start_periodic_and_wait`] plus [`Self::fetch`] for periodic
/// acquisition.
///
/// Use the more specialized methods only when you need a specific tradeoff:
///
/// - [`Self::measure_raw`] if you want raw `u16` words or integer-only
///   conversion via [`RawMeasurement::to_fixed_point`].
/// - [`Self::measure_temperature`] or [`Self::measure_temperature_millicelsius`]
///   if humidity is not needed and you want a shorter read transaction.
/// - `*_low_voltage` variants when VDD is below 2.4 V and the longer datasheet
///   conversion delays must be used.
/// - `*_with_clock_stretching` variants only when the I2C controller supports
///   sensor-driven clock stretching.
/// - `_and_wait` configuration methods when you want the driver to enforce the
///   datasheet's required 1 ms command gap.
#[derive(Debug)]
pub struct Sht3x<I2C> {
    i2c: I2C,
    address: u8,
}

impl<I2C> Sht3x<I2C> {
    /// Creates a driver using the default `0x44` address.
    #[must_use]
    pub fn new(i2c: I2C) -> Self {
        Self::with_address(i2c, Address::DEFAULT)
    }

    /// Creates a driver using the selected 7-bit address.
    #[must_use]
    pub fn with_address(i2c: I2C, address: Address) -> Self {
        Self {
            i2c,
            address: address.as_u8(),
        }
    }

    /// Returns the configured 7-bit I2C address.
    #[must_use]
    pub const fn address(&self) -> u8 {
        self.address
    }

    /// Releases the I2C bus.
    #[must_use]
    pub fn release(self) -> I2C {
        self.i2c
    }
}

impl<I2C> From<I2C> for Sht3x<I2C> {
    fn from(i2c: I2C) -> Self {
        Self::new(i2c)
    }
}

impl<I2C> Sht3x<I2C>
where
    I2C: I2c<SevenBitAddress>,
{
    /// Triggers one measurement without clock stretching and waits for it.
    pub fn measure<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<Measurement, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_raw(delay, repeatability)
            .map(RawMeasurement::to_measurement)
    }

    /// Triggers one measurement without clock stretching and waits for it.
    pub fn measure_raw<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_raw_after_delay(delay, repeatability, repeatability.delay_ms())
    }

    /// Triggers one measurement and reads only the temperature word.
    ///
    /// The read transfer requests only the first three bytes
    /// `(temperature MSB, temperature LSB, CRC)`, aborting before humidity to
    /// reduce I2C bus traffic.
    pub fn measure_temperature<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<f32, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_temperature_raw(delay, repeatability)
            .map(temperature_celsius_from_raw)
    }

    /// Triggers one measurement and reads only temperature in millidegrees Celsius.
    ///
    /// This is an integer-only alternative to [`Self::measure_temperature`].
    pub fn measure_temperature_millicelsius<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<i32, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_temperature_raw(delay, repeatability)
            .map(temperature_millicelsius_from_raw)
    }

    /// Triggers one measurement and reads only the raw temperature word.
    pub fn measure_temperature_raw<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<u16, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_temperature_raw_after_delay(delay, repeatability, repeatability.delay_ms())
    }

    /// Triggers one measurement without clock stretching at VDD below 2.4 V.
    pub fn measure_low_voltage<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<Measurement, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_raw_low_voltage(delay, repeatability)
            .map(RawMeasurement::to_measurement)
    }

    /// Triggers one raw measurement without clock stretching at VDD below 2.4 V.
    pub fn measure_raw_low_voltage<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_raw_after_delay(delay, repeatability, repeatability.low_voltage_delay_ms())
    }

    /// Triggers one low-voltage measurement and reads only the temperature word.
    pub fn measure_temperature_raw_low_voltage<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<u16, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_temperature_raw_after_delay(
            delay,
            repeatability,
            repeatability.low_voltage_delay_ms(),
        )
    }

    /// Triggers one measurement with clock stretching enabled.
    ///
    /// The I2C implementation must support clock stretching. ESP HAL I2C does.
    pub fn measure_with_clock_stretching(
        &mut self,
        repeatability: Repeatability,
    ) -> Result<Measurement, I2C::Error> {
        self.measure_raw_with_clock_stretching(repeatability)
            .map(RawMeasurement::to_measurement)
    }

    /// Triggers one raw measurement with clock stretching enabled.
    pub fn measure_raw_with_clock_stretching(
        &mut self,
        repeatability: Repeatability,
    ) -> Result<RawMeasurement, I2C::Error> {
        self.write_command(repeatability.single_shot_command(true))?;
        self.read_raw_measurement()
    }

    /// Triggers one clock-stretched measurement and reads only the temperature word.
    pub fn measure_temperature_raw_with_clock_stretching(
        &mut self,
        repeatability: Repeatability,
    ) -> Result<u16, I2C::Error> {
        self.write_command(repeatability.single_shot_command(true))?;
        self.read_raw_temperature()
    }

    /// Starts periodic acquisition.
    ///
    /// The SHT3x-DIS needs a 1 ms gap before it can receive the next command.
    /// Use [`Self::start_periodic_and_wait`] if the driver should enforce it.
    pub fn start_periodic(
        &mut self,
        repeatability: Repeatability,
        rate: PeriodicRate,
    ) -> Result<(), I2C::Error> {
        self.write_command(rate.command(repeatability))
    }

    /// Starts periodic acquisition and waits for the required command gap.
    pub fn start_periodic_and_wait<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
        rate: PeriodicRate,
    ) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command_and_wait(rate.command(repeatability), delay)
    }

    /// Starts periodic acquisition with accelerated response time.
    ///
    /// The SHT3x-DIS needs a 1 ms gap before it can receive the next command.
    /// Use [`Self::start_art_and_wait`] if the driver should enforce it.
    pub fn start_art(&mut self) -> Result<(), I2C::Error> {
        self.write_command(CMD_ART)
    }

    /// Starts ART mode and waits for the required command gap.
    pub fn start_art_and_wait<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command_and_wait(CMD_ART, delay)
    }

    /// Fetches one data pair from periodic acquisition.
    ///
    /// If no periodic sample is ready yet, the sensor responds to the read
    /// header with NACK and this returns [`Error::NotReady`]. Other I2C
    /// failures are still returned as [`Error::I2c`].
    pub fn fetch(&mut self) -> Result<Measurement, I2C::Error> {
        self.fetch_raw().map(RawMeasurement::to_measurement)
    }

    /// Fetches one raw data pair from periodic acquisition.
    ///
    /// If no periodic sample is ready yet, the sensor responds to the read
    /// header with NACK and this returns [`Error::NotReady`]. Other I2C
    /// failures are still returned as [`Error::I2c`].
    pub fn fetch_raw(&mut self) -> Result<RawMeasurement, I2C::Error> {
        self.write_command(CMD_FETCH_DATA)?;
        self.read_raw_measurement().map_err(map_fetch_error)
    }

    /// Stops periodic acquisition.
    ///
    /// The break command returns the sensor to single-shot mode in 1 ms.
    pub fn stop_periodic<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(CMD_BREAK)?;
        delay.delay_ms(1);
        Ok(())
    }

    /// Performs a device-specific soft reset.
    ///
    /// The SHT3x-DIS soft reset time has a 1.5 ms maximum, so this waits 2 ms.
    pub fn soft_reset<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(CMD_SOFT_RESET)?;
        delay.delay_ms(2);
        Ok(())
    }

    /// Performs an I2C general-call reset.
    ///
    /// This can reset every compatible device on the shared bus segment that
    /// responds to the general-call reset sequence. The reset is functionally
    /// identical to the dedicated reset pin, so this waits 2 ms to cover the
    /// worst-case power-up time.
    pub fn general_call_reset<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.i2c
            .write(GENERAL_CALL_ADDRESS, &[0x06])
            .map_err(crate::Error::I2c)?;
        delay.delay_ms(2);
        Ok(())
    }

    /// Enables or disables the internal heater.
    ///
    /// The SHT3x-DIS needs a 1 ms gap before it can receive the next command.
    /// Use [`Self::set_heater_and_wait`] if the driver should enforce it.
    pub fn set_heater(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        let command = if enabled {
            CMD_HEATER_ENABLE
        } else {
            CMD_HEATER_DISABLE
        };
        self.write_command(command)
    }

    /// Enables or disables the internal heater and waits for the required command gap.
    pub fn set_heater_and_wait<D>(&mut self, delay: &mut D, enabled: bool) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        let command = if enabled {
            CMD_HEATER_ENABLE
        } else {
            CMD_HEATER_DISABLE
        };
        self.write_command_and_wait(command, delay)
    }

    /// Reads the status register.
    pub fn status(&mut self) -> Result<Status, I2C::Error> {
        let mut data = [0; 3];
        self.write_command(CMD_READ_STATUS)?;
        self.i2c
            .read(self.address, &mut data)
            .map_err(crate::Error::I2c)?;

        check_crc(DataWord::Status, data[0], data[1], data[2])?;
        Ok(Status(u16::from_be_bytes([data[0], data[1]])))
    }

    /// Clears status register flags.
    ///
    /// The SHT3x-DIS needs a 1 ms gap before it can receive the next command.
    /// Use [`Self::clear_status_and_wait`] if the driver should enforce it.
    pub fn clear_status(&mut self) -> Result<(), I2C::Error> {
        self.write_command(CMD_CLEAR_STATUS)
    }

    /// Clears status register flags and waits for the required command gap.
    pub fn clear_status_and_wait<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command_and_wait(CMD_CLEAR_STATUS, delay)
    }

    fn write_command(&mut self, command: u16) -> Result<(), I2C::Error> {
        self.i2c
            .write(self.address, &command.to_be_bytes())
            .map_err(crate::Error::I2c)
    }

    fn write_command_and_wait<D>(&mut self, command: u16, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(command)?;
        delay.delay_ms(COMMAND_DELAY_MS);
        Ok(())
    }

    fn measure_raw_after_delay<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
        delay_ms: u32,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(repeatability.single_shot_command(false))?;
        delay.delay_ms(delay_ms);
        self.read_raw_measurement()
    }

    fn measure_temperature_raw_after_delay<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
        delay_ms: u32,
    ) -> Result<u16, I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(repeatability.single_shot_command(false))?;
        delay.delay_ms(delay_ms);
        self.read_raw_temperature()
    }

    fn read_raw_measurement(&mut self) -> Result<RawMeasurement, I2C::Error> {
        let mut data = [0; 6];
        self.i2c
            .read(self.address, &mut data)
            .map_err(crate::Error::I2c)?;
        parse_raw_measurement(data)
    }

    fn read_raw_temperature(&mut self) -> Result<u16, I2C::Error> {
        let mut data = [0; 3];
        self.i2c
            .read(self.address, &mut data)
            .map_err(crate::Error::I2c)?;
        parse_raw_temperature(data)
    }
}

#[cfg(feature = "async")]
impl<I2C> Sht3x<I2C>
where
    I2C: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
{
    /// Async version of [`Self::measure`].
    pub async fn measure_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<Measurement, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.measure_raw_async(delay, repeatability)
            .await
            .map(RawMeasurement::to_measurement)
    }

    /// Async version of [`Self::measure_raw`].
    pub async fn measure_raw_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.measure_raw_after_delay_async(delay, repeatability, repeatability.delay_ms())
            .await
    }

    /// Async version of [`Self::measure_temperature`].
    pub async fn measure_temperature_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<f32, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.measure_temperature_raw_async(delay, repeatability)
            .await
            .map(temperature_celsius_from_raw)
    }

    /// Async version of [`Self::measure_temperature_millicelsius`].
    pub async fn measure_temperature_millicelsius_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<i32, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.measure_temperature_raw_async(delay, repeatability)
            .await
            .map(temperature_millicelsius_from_raw)
    }

    /// Async version of [`Self::measure_temperature_raw`].
    pub async fn measure_temperature_raw_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<u16, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.measure_temperature_raw_after_delay_async(
            delay,
            repeatability,
            repeatability.delay_ms(),
        )
        .await
    }

    /// Async version of [`Self::measure_low_voltage`].
    pub async fn measure_low_voltage_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<Measurement, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.measure_raw_low_voltage_async(delay, repeatability)
            .await
            .map(RawMeasurement::to_measurement)
    }

    /// Async version of [`Self::measure_raw_low_voltage`].
    pub async fn measure_raw_low_voltage_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.measure_raw_after_delay_async(
            delay,
            repeatability,
            repeatability.low_voltage_delay_ms(),
        )
        .await
    }

    /// Async version of [`Self::measure_temperature_raw_low_voltage`].
    pub async fn measure_temperature_raw_low_voltage_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<u16, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.measure_temperature_raw_after_delay_async(
            delay,
            repeatability,
            repeatability.low_voltage_delay_ms(),
        )
        .await
    }

    /// Async version of [`Self::measure_with_clock_stretching`].
    pub async fn measure_with_clock_stretching_async(
        &mut self,
        repeatability: Repeatability,
    ) -> Result<Measurement, I2C::Error> {
        self.measure_raw_with_clock_stretching_async(repeatability)
            .await
            .map(RawMeasurement::to_measurement)
    }

    /// Async version of [`Self::measure_raw_with_clock_stretching`].
    pub async fn measure_raw_with_clock_stretching_async(
        &mut self,
        repeatability: Repeatability,
    ) -> Result<RawMeasurement, I2C::Error> {
        self.write_command_async(repeatability.single_shot_command(true))
            .await?;
        self.read_raw_measurement_async().await
    }

    /// Async version of [`Self::measure_temperature_raw_with_clock_stretching`].
    pub async fn measure_temperature_raw_with_clock_stretching_async(
        &mut self,
        repeatability: Repeatability,
    ) -> Result<u16, I2C::Error> {
        self.write_command_async(repeatability.single_shot_command(true))
            .await?;
        self.read_raw_temperature_async().await
    }

    /// Async version of [`Self::start_periodic`].
    pub async fn start_periodic_async(
        &mut self,
        repeatability: Repeatability,
        rate: PeriodicRate,
    ) -> Result<(), I2C::Error> {
        self.write_command_async(rate.command(repeatability)).await
    }

    /// Async version of [`Self::start_periodic_and_wait`].
    pub async fn start_periodic_and_wait_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
        rate: PeriodicRate,
    ) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_and_wait_async(rate.command(repeatability), delay)
            .await
    }

    /// Async version of [`Self::start_art`].
    pub async fn start_art_async(&mut self) -> Result<(), I2C::Error> {
        self.write_command_async(CMD_ART).await
    }

    /// Async version of [`Self::start_art_and_wait`].
    pub async fn start_art_and_wait_async<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_and_wait_async(CMD_ART, delay).await
    }

    /// Async version of [`Self::fetch`].
    ///
    /// If no periodic sample is ready yet, the sensor responds to the read
    /// header with NACK and this returns [`Error::NotReady`]. Other I2C
    /// failures are still returned as [`Error::I2c`].
    pub async fn fetch_async(&mut self) -> Result<Measurement, I2C::Error> {
        self.fetch_raw_async()
            .await
            .map(RawMeasurement::to_measurement)
    }

    /// Async version of [`Self::fetch_raw`].
    ///
    /// If no periodic sample is ready yet, the sensor responds to the read
    /// header with NACK and this returns [`Error::NotReady`]. Other I2C
    /// failures are still returned as [`Error::I2c`].
    pub async fn fetch_raw_async(&mut self) -> Result<RawMeasurement, I2C::Error> {
        self.write_command_async(CMD_FETCH_DATA).await?;
        self.read_raw_measurement_async()
            .await
            .map_err(map_fetch_error)
    }

    /// Async version of [`Self::stop_periodic`].
    pub async fn stop_periodic_async<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_async(CMD_BREAK).await?;
        delay.delay_ms(COMMAND_DELAY_MS).await;
        Ok(())
    }

    /// Async version of [`Self::soft_reset`].
    pub async fn soft_reset_async<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_async(CMD_SOFT_RESET).await?;
        delay.delay_ms(2).await;
        Ok(())
    }

    /// Async version of [`Self::general_call_reset`].
    pub async fn general_call_reset_async<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.i2c
            .write(GENERAL_CALL_ADDRESS, &[0x06])
            .await
            .map_err(crate::Error::I2c)?;
        delay.delay_ms(2).await;
        Ok(())
    }

    /// Async version of [`Self::set_heater`].
    pub async fn set_heater_async(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        let command = if enabled {
            CMD_HEATER_ENABLE
        } else {
            CMD_HEATER_DISABLE
        };
        self.write_command_async(command).await
    }

    /// Async version of [`Self::set_heater_and_wait`].
    pub async fn set_heater_and_wait_async<D>(
        &mut self,
        delay: &mut D,
        enabled: bool,
    ) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        let command = if enabled {
            CMD_HEATER_ENABLE
        } else {
            CMD_HEATER_DISABLE
        };
        self.write_command_and_wait_async(command, delay).await
    }

    /// Async version of [`Self::status`].
    pub async fn status_async(&mut self) -> Result<Status, I2C::Error> {
        let mut data = [0; 3];
        self.write_command_async(CMD_READ_STATUS).await?;
        self.i2c
            .read(self.address, &mut data)
            .await
            .map_err(crate::Error::I2c)?;

        check_crc(DataWord::Status, data[0], data[1], data[2])?;
        Ok(Status(u16::from_be_bytes([data[0], data[1]])))
    }

    /// Async version of [`Self::clear_status`].
    pub async fn clear_status_async(&mut self) -> Result<(), I2C::Error> {
        self.write_command_async(CMD_CLEAR_STATUS).await
    }

    /// Async version of [`Self::clear_status_and_wait`].
    pub async fn clear_status_and_wait_async<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_and_wait_async(CMD_CLEAR_STATUS, delay)
            .await
    }

    async fn write_command_async(&mut self, command: u16) -> Result<(), I2C::Error> {
        self.i2c
            .write(self.address, &command.to_be_bytes())
            .await
            .map_err(crate::Error::I2c)
    }

    async fn write_command_and_wait_async<D>(
        &mut self,
        command: u16,
        delay: &mut D,
    ) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_async(command).await?;
        delay.delay_ms(COMMAND_DELAY_MS).await;
        Ok(())
    }

    async fn measure_raw_after_delay_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
        delay_ms: u32,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_async(repeatability.single_shot_command(false))
            .await?;
        delay.delay_ms(delay_ms).await;
        self.read_raw_measurement_async().await
    }

    async fn measure_temperature_raw_after_delay_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
        delay_ms: u32,
    ) -> Result<u16, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_async(repeatability.single_shot_command(false))
            .await?;
        delay.delay_ms(delay_ms).await;
        self.read_raw_temperature_async().await
    }

    async fn read_raw_measurement_async(&mut self) -> Result<RawMeasurement, I2C::Error> {
        let mut data = [0; 6];
        self.i2c
            .read(self.address, &mut data)
            .await
            .map_err(crate::Error::I2c)?;
        parse_raw_measurement(data)
    }

    async fn read_raw_temperature_async(&mut self) -> Result<u16, I2C::Error> {
        let mut data = [0; 3];
        self.i2c
            .read(self.address, &mut data)
            .await
            .map_err(crate::Error::I2c)?;
        parse_raw_temperature(data)
    }
}
