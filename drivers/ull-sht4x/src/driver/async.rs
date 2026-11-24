use crate::types::{
    HeaterDuration, HeaterPower, Measurement, RawMeasurement, Repeatability, Result,
    heater_command, parse_raw_measurement, parse_serial_number,
};

use super::{CMD_READ_SERIAL, CMD_SOFT_RESET, SOFT_RESET_DELAY_MS, Sht4x};

impl<I2C> Sht4x<I2C>
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
        self.write_command_async(repeatability.command()).await?;
        delay.delay_ms(repeatability.delay_ms()).await;
        self.read_measurement_async().await
    }

    /// Async version of [`Self::serial_number`].
    pub async fn serial_number_async(&mut self) -> Result<u32, I2C::Error> {
        self.write_command_async(CMD_READ_SERIAL).await?;
        let mut data = [0u8; 6];
        self.i2c
            .read(self.address, &mut data)
            .await
            .map_err(crate::Error::Bus)?;
        parse_serial_number(data)
    }

    /// Async version of [`Self::soft_reset`].
    pub async fn soft_reset_async<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_async(CMD_SOFT_RESET).await?;
        delay.delay_ms(SOFT_RESET_DELAY_MS).await;
        Ok(())
    }

    /// Async version of [`Self::measure_with_heater_raw`].
    pub async fn measure_with_heater_raw_async<D>(
        &mut self,
        delay: &mut D,
        power: HeaterPower,
        duration: HeaterDuration,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_async(heater_command(power, duration))
            .await?;
        delay.delay_ms(duration.delay_ms()).await;
        self.read_measurement_async().await
    }

    /// Async version of [`Self::write_command`].
    pub async fn write_command_async(&mut self, command: u8) -> Result<(), I2C::Error> {
        self.i2c
            .write(self.address, &[command])
            .await
            .map_err(crate::Error::Bus)
    }

    /// Async version of [`Self::read_response_raw`].
    pub async fn read_response_raw_async(&mut self) -> Result<[u8; 6], I2C::Error> {
        let mut data = [0u8; 6];
        self.i2c
            .read(self.address, &mut data)
            .await
            .map_err(crate::Error::Bus)?;
        Ok(data)
    }

    async fn read_measurement_async(&mut self) -> Result<RawMeasurement, I2C::Error> {
        let data = self.read_response_raw_async().await?;
        parse_raw_measurement(data)
    }
}
