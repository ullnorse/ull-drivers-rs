use crate::types::{
    Measurement, PeriodicRate, RawMeasurement, Repeatability, Result, Status, map_fetch_error,
    parse_raw_measurement, parse_raw_temperature, temperature_celsius_from_raw,
    temperature_millicelsius_from_raw,
};

use super::{
    ArtMode, CMD_ART, CMD_BREAK, CMD_CLEAR_STATUS, CMD_FETCH_DATA, CMD_READ_STATUS, CMD_SOFT_RESET,
    COMMAND_DELAY_MS, GENERAL_CALL_ADDRESS, PeriodicMode, Sht3x, SingleShotMode, heater_command,
    parse_status,
};

impl<I2C> Sht3x<I2C, SingleShotMode>
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
        mut self,
        repeatability: Repeatability,
        rate: PeriodicRate,
    ) -> Result<Sht3x<I2C, PeriodicMode>, I2C::Error> {
        self.write_command_async(rate.command(repeatability))
            .await?;
        Ok(self.into_mode())
    }

    /// Async version of [`Self::start_periodic_and_wait`].
    pub async fn start_periodic_and_wait_async<D>(
        mut self,
        delay: &mut D,
        repeatability: Repeatability,
        rate: PeriodicRate,
    ) -> Result<Sht3x<I2C, PeriodicMode>, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_and_wait_async(rate.command(repeatability), delay)
            .await?;
        Ok(self.into_mode())
    }

    /// Async version of [`Self::start_art`].
    pub async fn start_art_async(mut self) -> Result<Sht3x<I2C, ArtMode>, I2C::Error> {
        self.write_command_async(CMD_ART).await?;
        Ok(self.into_mode())
    }

    /// Async version of [`Self::start_art_and_wait`].
    pub async fn start_art_and_wait_async<D>(
        mut self,
        delay: &mut D,
    ) -> Result<Sht3x<I2C, ArtMode>, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_and_wait_async(CMD_ART, delay).await?;
        Ok(self.into_mode())
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
            .map_err(crate::Error::Bus)?;
        delay.delay_ms(2).await;
        Ok(())
    }

    /// Async version of [`Self::set_heater`].
    pub async fn set_heater_async(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        self.write_command_async(heater_command(enabled)).await
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
        self.write_command_and_wait_async(heater_command(enabled), delay)
            .await
    }

    /// Async version of [`Self::status`].
    pub async fn status_async(&mut self) -> Result<Status, I2C::Error> {
        let mut data = [0; 3];
        self.write_command_async(CMD_READ_STATUS).await?;
        self.i2c
            .read(self.address, &mut data)
            .await
            .map_err(crate::Error::Bus)?;

        parse_status(data)
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
}

impl<I2C> Sht3x<I2C, PeriodicMode>
where
    I2C: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
{
    /// Async version of [`Self::fetch`].
    pub async fn fetch_async(&mut self) -> Result<Measurement, I2C::Error> {
        self.fetch_raw_async()
            .await
            .map(RawMeasurement::to_measurement)
    }

    /// Async version of [`Self::fetch_raw`].
    pub async fn fetch_raw_async(&mut self) -> Result<RawMeasurement, I2C::Error> {
        self.fetch_raw_async_inner().await
    }

    /// Async version of [`Self::stop_periodic`].
    pub async fn stop_periodic_async<D>(
        self,
        delay: &mut D,
    ) -> Result<Sht3x<I2C, SingleShotMode>, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.stop_periodic_async_inner(delay).await
    }
}

impl<I2C> Sht3x<I2C, ArtMode>
where
    I2C: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
{
    /// Async version of [`Self::fetch`].
    pub async fn fetch_async(&mut self) -> Result<Measurement, I2C::Error> {
        self.fetch_raw_async()
            .await
            .map(RawMeasurement::to_measurement)
    }

    /// Async version of [`Self::fetch_raw`].
    pub async fn fetch_raw_async(&mut self) -> Result<RawMeasurement, I2C::Error> {
        self.fetch_raw_async_inner().await
    }

    /// Async version of [`Self::stop_periodic`].
    pub async fn stop_periodic_async<D>(
        self,
        delay: &mut D,
    ) -> Result<Sht3x<I2C, SingleShotMode>, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.stop_periodic_async_inner(delay).await
    }
}

impl<I2C, MODE> Sht3x<I2C, MODE>
where
    I2C: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
{
    async fn fetch_raw_async_inner(&mut self) -> Result<RawMeasurement, I2C::Error> {
        self.write_command_async(CMD_FETCH_DATA).await?;
        self.read_raw_measurement_async()
            .await
            .map_err(map_fetch_error)
    }

    async fn stop_periodic_async_inner<D>(
        mut self,
        delay: &mut D,
    ) -> Result<Sht3x<I2C, SingleShotMode>, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_async(CMD_BREAK).await?;
        delay.delay_ms(COMMAND_DELAY_MS).await;
        Ok(self.into_mode())
    }

    async fn write_command_async(&mut self, command: u16) -> Result<(), I2C::Error> {
        self.i2c
            .write(self.address, &command.to_be_bytes())
            .await
            .map_err(crate::Error::Bus)
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
            .map_err(crate::Error::Bus)?;
        parse_raw_measurement(data)
    }

    async fn read_raw_temperature_async(&mut self) -> Result<u16, I2C::Error> {
        let mut data = [0; 3];
        self.i2c
            .read(self.address, &mut data)
            .await
            .map_err(crate::Error::Bus)?;
        parse_raw_temperature(data)
    }
}
