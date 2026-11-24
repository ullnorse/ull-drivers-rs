use crate::types::{
    CONFIG_ALRT, CONFIG_ALSC, CONFIG_ATHD_MASK, CONFIG_SLEEP, Config, Error, Result,
    STATUS_ALERT_FLAGS, STATUS_RI, Status, VoltageAlert, charge_rate_milli_percent_per_hour,
    soc_hundredths, vcell_microvolts,
};

use super::{
    HIBRT_ALWAYS, HIBRT_DEFAULT, HIBRT_DISABLE, MODE_ENSLEEP, MODE_HIBSTAT, Max17048, REG_CONFIG,
    REG_CRATE, REG_HIBRT, REG_MODE, REG_SOC, REG_STATUS, REG_VALRT, REG_VCELL, REG_VERSION,
};

impl<I2C> Max17048<I2C>
where
    I2C: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
{
    /// Async version of [`Self::vcell_raw`].
    pub async fn vcell_raw_async(&mut self) -> Result<u16, I2C::Error> {
        self.read_register_async(REG_VCELL).await
    }

    /// Async version of [`Self::vcell_microvolts`].
    pub async fn vcell_microvolts_async(&mut self) -> Result<u32, I2C::Error> {
        self.vcell_raw_async().await.map(vcell_microvolts)
    }

    /// Async version of [`Self::vcell_millivolts`].
    pub async fn vcell_millivolts_async(&mut self) -> Result<u16, I2C::Error> {
        self.vcell_microvolts_async()
            .await
            .map(|uv| (uv / 1000) as u16)
    }

    /// Async version of [`Self::vcell_volts`].
    pub async fn vcell_volts_async(&mut self) -> Result<f32, I2C::Error> {
        self.vcell_raw_async()
            .await
            .map(|raw| raw as f32 * 78.125e-6)
    }

    /// Async version of [`Self::soc_raw`].
    pub async fn soc_raw_async(&mut self) -> Result<u16, I2C::Error> {
        self.read_register_async(REG_SOC).await
    }

    /// Async version of [`Self::soc_hundredths`].
    pub async fn soc_hundredths_async(&mut self) -> Result<u16, I2C::Error> {
        self.soc_raw_async().await.map(soc_hundredths)
    }

    /// Async version of [`Self::soc_percent`].
    pub async fn soc_percent_async(&mut self) -> Result<f32, I2C::Error> {
        self.soc_raw_async().await.map(|raw| raw as f32 / 256.0)
    }

    /// Async version of [`Self::charge_rate_raw`].
    pub async fn charge_rate_raw_async(&mut self) -> Result<i16, I2C::Error> {
        self.read_register_async(REG_CRATE)
            .await
            .map(|raw| raw as i16)
    }

    /// Async version of [`Self::charge_rate_milli_percent_per_hour`].
    pub async fn charge_rate_milli_percent_per_hour_async(&mut self) -> Result<i32, I2C::Error> {
        self.charge_rate_raw_async()
            .await
            .map(charge_rate_milli_percent_per_hour)
    }

    /// Async version of [`Self::charge_rate_percent_per_hour`].
    pub async fn charge_rate_percent_per_hour_async(&mut self) -> Result<f32, I2C::Error> {
        self.charge_rate_raw_async()
            .await
            .map(|raw| raw as f32 * 0.208)
    }

    /// Async version of [`Self::version`].
    pub async fn version_async(&mut self) -> Result<u16, I2C::Error> {
        self.read_register_async(REG_VERSION).await
    }

    /// Async version of [`Self::config`].
    pub async fn config_async(&mut self) -> Result<Config, I2C::Error> {
        self.read_register_async(REG_CONFIG).await.map(Config)
    }

    /// Async version of [`Self::set_low_soc_threshold`].
    pub async fn set_low_soc_threshold_async(&mut self, percent: u8) -> Result<(), I2C::Error> {
        if !(1..=32).contains(&percent) {
            return Err(Error::InvalidThreshold);
        }
        self.update_config_async(CONFIG_ATHD_MASK, u16::from(32 - percent))
            .await
    }

    /// Async version of [`Self::set_soc_change_alert`].
    pub async fn set_soc_change_alert_async(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        self.update_config_async(CONFIG_ALSC, if enabled { CONFIG_ALSC } else { 0 })
            .await
    }

    /// Async version of [`Self::set_voltage_alert`].
    pub async fn set_voltage_alert_async(&mut self, alert: VoltageAlert) -> Result<(), I2C::Error> {
        let raw = alert.to_register().ok_or(Error::InvalidThreshold)?;
        self.write_register_async(REG_VALRT, raw).await
    }

    /// Async version of [`Self::voltage_alert`].
    pub async fn voltage_alert_async(&mut self) -> Result<VoltageAlert, I2C::Error> {
        self.read_register_async(REG_VALRT)
            .await
            .map(VoltageAlert::from_register)
    }

    /// Async version of [`Self::status`].
    pub async fn status_async(&mut self) -> Result<Status, I2C::Error> {
        self.read_register_async(REG_STATUS).await.map(Status)
    }

    /// Async version of [`Self::alert_asserted`].
    pub async fn alert_asserted_async(&mut self) -> Result<bool, I2C::Error> {
        self.config_async().await.map(Config::alert)
    }

    /// Async version of [`Self::clear_alert`].
    pub async fn clear_alert_async(&mut self) -> Result<Status, I2C::Error> {
        let status = self.status_async().await?;
        self.write_register_async(REG_STATUS, status.0 & !STATUS_ALERT_FLAGS & 0xFF00)
            .await?;
        self.update_config_async(CONFIG_ALRT, 0).await?;
        Ok(status)
    }

    /// Async version of [`Self::clear_reset_indicator`].
    pub async fn clear_reset_indicator_async(&mut self) -> Result<(), I2C::Error> {
        let status = self.status_async().await?;
        self.write_register_async(REG_STATUS, status.0 & !STATUS_RI & 0xFF00)
            .await
    }

    /// Async version of [`Self::is_hibernating`].
    pub async fn is_hibernating_async(&mut self) -> Result<bool, I2C::Error> {
        self.read_register_async(REG_MODE)
            .await
            .map(|mode| mode & MODE_HIBSTAT != 0)
    }

    /// Async version of [`Self::enter_hibernate`].
    pub async fn enter_hibernate_async(&mut self) -> Result<(), I2C::Error> {
        self.write_register_async(REG_HIBRT, HIBRT_ALWAYS).await
    }

    /// Async version of [`Self::exit_hibernate`].
    pub async fn exit_hibernate_async(&mut self) -> Result<(), I2C::Error> {
        self.write_register_async(REG_HIBRT, HIBRT_DISABLE).await
    }

    /// Async version of [`Self::set_auto_hibernate`].
    pub async fn set_auto_hibernate_async(&mut self) -> Result<(), I2C::Error> {
        self.write_register_async(REG_HIBRT, HIBRT_DEFAULT).await
    }

    /// Async version of [`Self::enter_sleep`].
    pub async fn enter_sleep_async(&mut self) -> Result<(), I2C::Error> {
        self.write_register_async(REG_MODE, MODE_ENSLEEP).await?;
        self.update_config_async(CONFIG_SLEEP, CONFIG_SLEEP).await
    }

    /// Async version of [`Self::wake`].
    pub async fn wake_async(&mut self) -> Result<(), I2C::Error> {
        self.update_config_async(CONFIG_SLEEP, 0).await
    }

    /// Async version of [`Self::read_register`].
    pub async fn read_register_async(&mut self, register: u8) -> Result<u16, I2C::Error> {
        let mut data = [0u8; 2];
        self.i2c
            .write_read(self.address, &[register], &mut data)
            .await
            .map_err(Error::Bus)?;
        Ok(u16::from_be_bytes(data))
    }

    /// Async version of [`Self::write_register`].
    pub async fn write_register_async(
        &mut self,
        register: u8,
        value: u16,
    ) -> Result<(), I2C::Error> {
        let [msb, lsb] = value.to_be_bytes();
        self.i2c
            .write(self.address, &[register, msb, lsb])
            .await
            .map_err(Error::Bus)
    }

    /// Read-modify-write on CONFIG: `new = (old & !mask) | (value & mask)`.
    async fn update_config_async(&mut self, mask: u16, value: u16) -> Result<(), I2C::Error> {
        let config = self.read_register_async(REG_CONFIG).await?;
        self.write_register_async(REG_CONFIG, (config & !mask) | (value & mask))
            .await
    }
}
