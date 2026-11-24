use crate::types::{
    AllInterruptSource, Config, Error, InterruptPinConfig, RawAcceleration, Result, Status,
    WHO_AM_I, WakeUpConfig, WakeUpSource,
};

use super::{
    CTRL1_ODR_MASK, CTRL2_BDU, CTRL2_IF_ADD_INC, CTRL2_SOFT_RESET, CTRL3_PIN_MASK,
    CTRL3_SLP_MODE_1, CTRL3_SLP_MODE_SEL, CTRL4_INT1_WU, CTRL7_INTERRUPTS_ENABLE, Lis2dw12,
    REG_ALL_INT_SRC, REG_CTRL1, REG_CTRL2, REG_CTRL3, REG_CTRL4_INT1_PAD, REG_CTRL6, REG_CTRL7,
    REG_OUT_T, REG_OUT_X_L, REG_STATUS, REG_WAKE_UP_DUR, REG_WAKE_UP_SRC, REG_WAKE_UP_THS,
    REG_WHO_AM_I, SOFT_RESET_POLL_ATTEMPTS, SOFT_RESET_POLL_MS,
};

impl<I2C> Lis2dw12<I2C>
where
    I2C: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
{
    /// Async version of [`Self::who_am_i`].
    pub async fn who_am_i_async(&mut self) -> Result<u8, I2C::Error> {
        self.read_register_async(REG_WHO_AM_I).await
    }

    /// Async version of [`Self::check_who_am_i`].
    pub async fn check_who_am_i_async(&mut self) -> Result<(), I2C::Error> {
        match self.who_am_i_async().await? {
            WHO_AM_I => Ok(()),
            other => Err(Error::UnexpectedChipId(other)),
        }
    }

    /// Async version of [`Self::soft_reset`].
    pub async fn soft_reset_async<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_register_async(REG_CTRL2, CTRL2_SOFT_RESET | CTRL2_IF_ADD_INC)
            .await?;
        for _ in 0..SOFT_RESET_POLL_ATTEMPTS {
            delay.delay_ms(SOFT_RESET_POLL_MS).await;
            if self.read_register_async(REG_CTRL2).await? & CTRL2_SOFT_RESET == 0 {
                return Ok(());
            }
        }
        Err(Error::ResetTimeout)
    }

    /// Async version of [`Self::init`].
    pub async fn init_async<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.soft_reset_async(delay).await?;
        self.check_who_am_i_async().await
    }

    /// Async version of [`Self::configure`].
    pub async fn configure_async(&mut self, config: &Config) -> Result<(), I2C::Error> {
        self.write_register_async(REG_CTRL6, config.ctrl6()).await?;
        self.write_register_async(REG_CTRL1, config.ctrl1()).await
    }

    /// Async version of [`Self::read_config`].
    pub async fn read_config_async(&mut self) -> Result<Config, I2C::Error> {
        let ctrl1 = self.read_register_async(REG_CTRL1).await?;
        let ctrl6 = self.read_register_async(REG_CTRL6).await?;
        Config::from_registers(ctrl1, ctrl6).ok_or(Error::InvalidConfig)
    }

    /// Async version of [`Self::power_down`].
    pub async fn power_down_async(&mut self) -> Result<(), I2C::Error> {
        let ctrl1 = self.read_register_async(REG_CTRL1).await?;
        self.write_register_async(REG_CTRL1, ctrl1 & !CTRL1_ODR_MASK)
            .await
    }

    /// Async version of [`Self::set_block_data_update`].
    pub async fn set_block_data_update_async(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        self.update_register_async(REG_CTRL2, CTRL2_BDU, if enabled { CTRL2_BDU } else { 0 })
            .await
    }

    /// Async version of [`Self::trigger_single_conversion`].
    pub async fn trigger_single_conversion_async(&mut self) -> Result<(), I2C::Error> {
        let bits = CTRL3_SLP_MODE_SEL | CTRL3_SLP_MODE_1;
        self.update_register_async(REG_CTRL3, bits, bits).await
    }

    /// Async version of [`Self::status`].
    pub async fn status_async(&mut self) -> Result<Status, I2C::Error> {
        self.read_register_async(REG_STATUS).await.map(Status)
    }

    /// Async version of [`Self::read_acceleration`].
    pub async fn read_acceleration_async(&mut self) -> Result<RawAcceleration, I2C::Error> {
        let mut data = [0u8; 6];
        self.read_registers_async(REG_OUT_X_L, &mut data).await?;
        Ok(RawAcceleration::from_bytes(data))
    }

    /// Async version of [`Self::read_acceleration_mg`].
    pub async fn read_acceleration_mg_async(
        &mut self,
        config: &Config,
    ) -> Result<[f32; 3], I2C::Error> {
        self.read_acceleration_async()
            .await
            .map(|raw| raw.to_milli_g(config.full_scale, config.resolution()))
    }

    /// Async version of [`Self::read_temperature_raw`].
    pub async fn read_temperature_raw_async(&mut self) -> Result<i8, I2C::Error> {
        self.read_register_async(REG_OUT_T)
            .await
            .map(|value| value as i8)
    }

    /// Async version of [`Self::read_temperature_celsius`].
    pub async fn read_temperature_celsius_async(&mut self) -> Result<i16, I2C::Error> {
        self.read_temperature_raw_async()
            .await
            .map(|raw| 25 + i16::from(raw))
    }

    /// Async version of [`Self::interrupt_pin_config`].
    pub async fn interrupt_pin_config_async(&mut self) -> Result<InterruptPinConfig, I2C::Error> {
        self.read_register_async(REG_CTRL3)
            .await
            .map(InterruptPinConfig::from_ctrl3)
    }

    /// Async version of [`Self::set_interrupt_pin_config`].
    pub async fn set_interrupt_pin_config_async(
        &mut self,
        config: &InterruptPinConfig,
    ) -> Result<(), I2C::Error> {
        self.update_register_async(REG_CTRL3, CTRL3_PIN_MASK, config.ctrl3_bits())
            .await
    }

    /// Async version of [`Self::enable_wake_up_interrupt`].
    pub async fn enable_wake_up_interrupt_async(
        &mut self,
        config: &WakeUpConfig,
    ) -> Result<(), I2C::Error> {
        let (threshold, duration) = config.registers().ok_or(Error::InvalidConfig)?;
        self.write_register_async(REG_WAKE_UP_THS, threshold)
            .await?;
        self.write_register_async(REG_WAKE_UP_DUR, duration).await?;
        self.update_register_async(REG_CTRL4_INT1_PAD, CTRL4_INT1_WU, CTRL4_INT1_WU)
            .await?;
        self.update_register_async(REG_CTRL7, CTRL7_INTERRUPTS_ENABLE, CTRL7_INTERRUPTS_ENABLE)
            .await?;
        self.clear_interrupts_async().await?;
        Ok(())
    }

    /// Async version of [`Self::disable_wake_up_interrupt`].
    pub async fn disable_wake_up_interrupt_async(&mut self) -> Result<(), I2C::Error> {
        self.update_register_async(REG_CTRL4_INT1_PAD, CTRL4_INT1_WU, 0)
            .await
    }

    /// Async version of [`Self::read_wake_up_source`].
    pub async fn read_wake_up_source_async(&mut self) -> Result<WakeUpSource, I2C::Error> {
        self.read_register_async(REG_WAKE_UP_SRC)
            .await
            .map(WakeUpSource)
    }

    /// Async version of [`Self::clear_interrupts`].
    pub async fn clear_interrupts_async(&mut self) -> Result<AllInterruptSource, I2C::Error> {
        self.read_register_async(REG_ALL_INT_SRC)
            .await
            .map(AllInterruptSource)
    }

    /// Read-modify-write: `register = (register & !mask) | (value & mask)`.
    async fn update_register_async(
        &mut self,
        register: u8,
        mask: u8,
        value: u8,
    ) -> Result<(), I2C::Error> {
        let current = self.read_register_async(register).await?;
        self.write_register_async(register, (current & !mask) | (value & mask))
            .await
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
