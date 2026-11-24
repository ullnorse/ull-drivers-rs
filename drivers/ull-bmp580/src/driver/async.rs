use embedded_hal::i2c::{Error as _, ErrorKind};

use crate::types::{
    CHIP_ID, Config, Error, InterruptConfig, InterruptStatus, PWR_MODE_MASK, PowerMode,
    RawMeasurement, Result, forced_measurement_delay_ms, nvm_ok,
};

use super::{
    Bmp580, CMD_SOFT_RESET, INT_SOURCE_DRDY, OSR_EFF_ODR_VALID, REG_CHIP_ID, REG_CMD,
    REG_DATA_START, REG_DSP_IIR, REG_INT_CONFIG, REG_INT_SOURCE, REG_INT_STATUS, REG_ODR_CONFIG,
    REG_OSR_CONFIG, REG_OSR_EFF, REG_STATUS, SOFT_RESET_DELAY_MS, STANDBY_DELAY_MS,
};

impl<I2C> Bmp580<I2C>
where
    I2C: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
{
    /// Async version of [`Self::chip_id`].
    pub async fn chip_id_async(&mut self) -> Result<u8, I2C::Error> {
        self.read_register_async(REG_CHIP_ID).await
    }

    /// Async version of [`Self::check_chip_id`].
    pub async fn check_chip_id_async(&mut self) -> Result<(), I2C::Error> {
        match self.chip_id_async().await? {
            CHIP_ID => Ok(()),
            other => Err(Error::UnexpectedChipId(other)),
        }
    }

    /// Async version of [`Self::soft_reset`].
    pub async fn soft_reset_async<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        match self
            .i2c
            .write(self.address, &[REG_CMD, CMD_SOFT_RESET])
            .await
        {
            Ok(()) => {}
            Err(error) if matches!(error.kind(), ErrorKind::NoAcknowledge(_)) => {}
            Err(error) => return Err(Error::Bus(error)),
        }
        delay.delay_ms(SOFT_RESET_DELAY_MS).await;
        Ok(())
    }

    /// Async version of [`Self::init`].
    pub async fn init_async<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.soft_reset_async(delay).await?;
        self.check_chip_id_async().await?;
        if !nvm_ok(self.read_register_async(REG_STATUS).await?) {
            return Err(Error::Nvm);
        }
        self.interrupt_status_async().await?;
        Ok(())
    }

    /// Async version of [`Self::power_mode`].
    pub async fn power_mode_async(&mut self) -> Result<PowerMode, I2C::Error> {
        self.read_register_async(REG_ODR_CONFIG)
            .await
            .map(PowerMode::from_bits)
    }

    /// Async version of [`Self::set_power_mode`].
    pub async fn set_power_mode_async(&mut self, mode: PowerMode) -> Result<(), I2C::Error> {
        let odr = self.read_register_async(REG_ODR_CONFIG).await?;
        self.write_register_async(REG_ODR_CONFIG, (odr & !PWR_MODE_MASK) | mode as u8)
            .await
    }

    /// Async version of [`Self::set_standby`].
    pub async fn set_standby_async<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.set_power_mode_async(PowerMode::Standby).await?;
        delay.delay_ms(STANDBY_DELAY_MS).await;
        Ok(())
    }

    /// Async version of [`Self::configure`].
    pub async fn configure_async<D>(
        &mut self,
        delay: &mut D,
        config: &Config,
    ) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        if self.power_mode_async().await? != PowerMode::Standby {
            self.set_standby_async(delay).await?;
        }
        self.write_register_async(REG_DSP_IIR, config.iir_register())
            .await?;
        self.write_register_async(REG_OSR_CONFIG, config.osr_register())
            .await?;
        self.write_register_async(REG_ODR_CONFIG, config.odr_register(PowerMode::Standby))
            .await
    }

    /// Async version of [`Self::odr_osr_valid`].
    pub async fn odr_osr_valid_async(&mut self) -> Result<bool, I2C::Error> {
        self.read_register_async(REG_OSR_EFF)
            .await
            .map(|value| value & OSR_EFF_ODR_VALID != 0)
    }

    /// Async version of [`Self::measure_forced`].
    pub async fn measure_forced_async<D>(
        &mut self,
        delay: &mut D,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        let mut regs = [0u8; 2];
        self.read_registers_async(REG_OSR_CONFIG, &mut regs).await?;
        let [osr, odr] = regs;
        let standby = odr & !PWR_MODE_MASK;
        if odr & PWR_MODE_MASK != PowerMode::Standby as u8 {
            self.write_register_async(REG_ODR_CONFIG, standby).await?;
            delay.delay_ms(STANDBY_DELAY_MS).await;
        }
        self.write_register_async(REG_ODR_CONFIG, standby | PowerMode::Forced as u8)
            .await?;
        delay.delay_ms(forced_measurement_delay_ms(osr)).await;
        self.read_measurement_async().await
    }

    /// Async version of [`Self::read_measurement`].
    pub async fn read_measurement_async(&mut self) -> Result<RawMeasurement, I2C::Error> {
        let mut data = [0u8; 6];
        self.read_registers_async(REG_DATA_START, &mut data).await?;
        Ok(RawMeasurement::from_bytes(data))
    }

    /// Async version of [`Self::read_temperature_raw`].
    pub async fn read_temperature_raw_async(&mut self) -> Result<i32, I2C::Error> {
        let mut data = [0u8; 3];
        self.read_registers_async(REG_DATA_START, &mut data).await?;
        Ok(crate::types::sign_extend_24(data[0], data[1], data[2]))
    }

    /// Async version of [`Self::interrupt_status`].
    pub async fn interrupt_status_async(&mut self) -> Result<InterruptStatus, I2C::Error> {
        self.read_register_async(REG_INT_STATUS)
            .await
            .map(InterruptStatus)
    }

    /// Async version of [`Self::interrupt_config`].
    pub async fn interrupt_config_async(&mut self) -> Result<(InterruptConfig, bool), I2C::Error> {
        self.read_register_async(REG_INT_CONFIG)
            .await
            .map(InterruptConfig::from_register)
    }

    /// Async version of [`Self::set_interrupt_config`].
    pub async fn set_interrupt_config_async(
        &mut self,
        config: &InterruptConfig,
        enabled: bool,
    ) -> Result<(), I2C::Error> {
        let value = config.to_register(enabled).ok_or(Error::InvalidConfig)?;
        self.write_register_async(REG_INT_CONFIG, value).await
    }

    /// Async version of [`Self::configure_data_ready_interrupt`].
    pub async fn configure_data_ready_interrupt_async(
        &mut self,
        config: &InterruptConfig,
    ) -> Result<(), I2C::Error> {
        let value = config.to_register(true).ok_or(Error::InvalidConfig)?;
        self.write_register_async(REG_INT_SOURCE, 0x00).await?;
        self.interrupt_status_async().await?;
        self.write_register_async(REG_INT_CONFIG, value).await?;
        self.write_register_async(REG_INT_SOURCE, INT_SOURCE_DRDY)
            .await
    }

    /// Async version of [`Self::disable_interrupts`].
    pub async fn disable_interrupts_async(&mut self) -> Result<(), I2C::Error> {
        self.write_register_async(REG_INT_SOURCE, 0x00).await
    }

    /// Async version of [`Self::read_register`].
    pub async fn read_register_async(&mut self, register: u8) -> Result<u8, I2C::Error> {
        let mut data = [0u8; 1];
        self.read_registers_async(register, &mut data).await?;
        Ok(data[0])
    }

    /// Async version of [`Self::read_registers`].
    pub async fn read_registers_async(
        &mut self,
        register: u8,
        buffer: &mut [u8],
    ) -> Result<(), I2C::Error> {
        self.i2c
            .write_read(self.address, &[register], buffer)
            .await
            .map_err(Error::Bus)
    }

    /// Async version of [`Self::write_register`].
    pub async fn write_register_async(
        &mut self,
        register: u8,
        value: u8,
    ) -> Result<(), I2C::Error> {
        self.i2c
            .write(self.address, &[register, value])
            .await
            .map_err(Error::Bus)
    }
}
