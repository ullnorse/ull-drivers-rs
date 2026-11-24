use embedded_hal::i2c::{I2c, SevenBitAddress};

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
    I2C: I2c<SevenBitAddress>,
{
    /// Reads the raw VCELL register (78.125 uV per LSb).
    pub fn vcell_raw(&mut self) -> Result<u16, I2C::Error> {
        self.read_register(REG_VCELL)
    }

    /// Reads the cell voltage in microvolts.
    pub fn vcell_microvolts(&mut self) -> Result<u32, I2C::Error> {
        self.vcell_raw().map(vcell_microvolts)
    }

    /// Reads the cell voltage in millivolts (truncated).
    pub fn vcell_millivolts(&mut self) -> Result<u16, I2C::Error> {
        self.vcell_microvolts().map(|uv| (uv / 1000) as u16)
    }

    /// Reads the cell voltage in volts.
    pub fn vcell_volts(&mut self) -> Result<f32, I2C::Error> {
        self.vcell_raw().map(|raw| raw as f32 * 78.125e-6)
    }

    /// Reads the raw SOC register (1 %/256 per LSb).
    pub fn soc_raw(&mut self) -> Result<u16, I2C::Error> {
        self.read_register(REG_SOC)
    }

    /// Reads the state of charge in hundredths of a percent (`5000` = 50.00 %).
    pub fn soc_hundredths(&mut self) -> Result<u16, I2C::Error> {
        self.soc_raw().map(soc_hundredths)
    }

    /// Reads the state of charge in percent.
    pub fn soc_percent(&mut self) -> Result<f32, I2C::Error> {
        self.soc_raw().map(|raw| raw as f32 / 256.0)
    }

    /// Reads the raw CRATE register as a signed value (0.208 %/hr per LSb).
    pub fn charge_rate_raw(&mut self) -> Result<i16, I2C::Error> {
        self.read_register(REG_CRATE).map(|raw| raw as i16)
    }

    /// Reads the charge rate in thousandths of a percent per hour. Negative
    /// while discharging.
    pub fn charge_rate_milli_percent_per_hour(&mut self) -> Result<i32, I2C::Error> {
        self.charge_rate_raw()
            .map(charge_rate_milli_percent_per_hour)
    }

    /// Reads the charge rate in percent per hour. Negative while discharging.
    pub fn charge_rate_percent_per_hour(&mut self) -> Result<f32, I2C::Error> {
        self.charge_rate_raw().map(|raw| raw as f32 * 0.208)
    }

    /// Reads the production version register.
    pub fn version(&mut self) -> Result<u16, I2C::Error> {
        self.read_register(REG_VERSION)
    }

    /// Reads the CONFIG register.
    pub fn config(&mut self) -> Result<Config, I2C::Error> {
        self.read_register(REG_CONFIG).map(Config)
    }

    /// Sets the low-SOC alert threshold (`CONFIG.ATHD`) to `percent`, 1..=32.
    ///
    /// The alert fires on a falling edge past the threshold. Other CONFIG bits
    /// (RCOMP, SLEEP, ALSC, ALRT) are preserved.
    pub fn set_low_soc_threshold(&mut self, percent: u8) -> Result<(), I2C::Error> {
        if !(1..=32).contains(&percent) {
            return Err(Error::InvalidThreshold);
        }
        self.update_config(CONFIG_ATHD_MASK, u16::from(32 - percent))
    }

    /// Enables or disables the alert on every 1 % SOC change (`CONFIG.ALSC`).
    pub fn set_soc_change_alert(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        self.update_config(CONFIG_ALSC, if enabled { CONFIG_ALSC } else { 0 })
    }

    /// Sets the voltage alert window (`VALRT.MIN` and `VALRT.MAX`).
    ///
    /// Returns [`Error::InvalidThreshold`] if a value exceeds 5100 mV.
    pub fn set_voltage_alert(&mut self, alert: VoltageAlert) -> Result<(), I2C::Error> {
        let raw = alert.to_register().ok_or(Error::InvalidThreshold)?;
        self.write_register(REG_VALRT, raw)
    }

    /// Reads the voltage alert window.
    pub fn voltage_alert(&mut self) -> Result<VoltageAlert, I2C::Error> {
        self.read_register(REG_VALRT)
            .map(VoltageAlert::from_register)
    }

    /// Reads the STATUS register (alert indicators).
    pub fn status(&mut self) -> Result<Status, I2C::Error> {
        self.read_register(REG_STATUS).map(Status)
    }

    /// Returns `true` while `CONFIG.ALRT` is set, which is when the ALRT pin
    /// is held low.
    pub fn alert_asserted(&mut self) -> Result<bool, I2C::Error> {
        self.config().map(Config::alert)
    }

    /// Clears the alert and deasserts the ALRT pin, returning the STATUS that
    /// was pending.
    ///
    /// Clears the alert indicators in STATUS (VH, VL, VR, HD, SC; EnVr and RI
    /// are preserved), then writes `CONFIG.ALRT = 0` with the rest of CONFIG
    /// unchanged. The pin only releases when `CONFIG.ALRT` is cleared; clearing
    /// STATUS alone is not enough.
    ///
    /// If the alert condition still holds (for example VCELL is still below
    /// `VALRT.MIN`), the IC may assert ALRT again on a later sample.
    pub fn clear_alert(&mut self) -> Result<Status, I2C::Error> {
        let status = self.status()?;
        self.write_register(REG_STATUS, status.0 & !STATUS_ALERT_FLAGS & 0xFF00)?;
        self.update_config(CONFIG_ALRT, 0)?;
        Ok(status)
    }

    /// Clears the reset indicator (RI) after the device has been configured.
    pub fn clear_reset_indicator(&mut self) -> Result<(), I2C::Error> {
        let status = self.status()?;
        self.write_register(REG_STATUS, status.0 & !STATUS_RI & 0xFF00)
    }

    /// Returns `true` if the IC is currently in hibernate mode (`MODE.HibStat`).
    pub fn is_hibernating(&mut self) -> Result<bool, I2C::Error> {
        self.read_register(REG_MODE)
            .map(|mode| mode & MODE_HIBSTAT != 0)
    }

    /// Forces hibernate mode (`HIBRT = 0xFFFF`). ADC and SOC update every 45 s.
    pub fn enter_hibernate(&mut self) -> Result<(), I2C::Error> {
        self.write_register(REG_HIBRT, HIBRT_ALWAYS)
    }

    /// Forces active mode by disabling hibernate (`HIBRT = 0x0000`).
    pub fn exit_hibernate(&mut self) -> Result<(), I2C::Error> {
        self.write_register(REG_HIBRT, HIBRT_DISABLE)
    }

    /// Restores automatic hibernate control (`HIBRT = 0x8030`, power-on value).
    pub fn set_auto_hibernate(&mut self) -> Result<(), I2C::Error> {
        self.write_register(REG_HIBRT, HIBRT_DEFAULT)
    }

    /// Enters sleep mode: sets `MODE.EnSleep`, then `CONFIG.SLEEP = 1`.
    ///
    /// The IC halts and cannot track self-discharge while asleep. Sleep does
    /// not clear alerts. Only `CONFIG.SLEEP = 0` wakes it (see [`Self::wake`]).
    pub fn enter_sleep(&mut self) -> Result<(), I2C::Error> {
        self.write_register(REG_MODE, MODE_ENSLEEP)?;
        self.update_config(CONFIG_SLEEP, CONFIG_SLEEP)
    }

    /// Wakes the IC from sleep mode (`CONFIG.SLEEP = 0`).
    pub fn wake(&mut self) -> Result<(), I2C::Error> {
        self.update_config(CONFIG_SLEEP, 0)
    }

    /// Raw escape hatch: reads one 16-bit register in a single transaction
    /// (write address, repeated start, read two bytes, MSB first).
    pub fn read_register(&mut self, register: u8) -> Result<u16, I2C::Error> {
        let mut data = [0u8; 2];
        self.i2c
            .write_read(self.address, &[register], &mut data)
            .map_err(Error::Bus)?;
        Ok(u16::from_be_bytes(data))
    }

    /// Raw escape hatch: writes one 16-bit register in a single transaction.
    ///
    /// The IC only accepts whole-word writes. Bits documented as read-only or
    /// don't-care are ignored by the IC.
    pub fn write_register(&mut self, register: u8, value: u16) -> Result<(), I2C::Error> {
        let [msb, lsb] = value.to_be_bytes();
        self.i2c
            .write(self.address, &[register, msb, lsb])
            .map_err(Error::Bus)
    }

    /// Read-modify-write on CONFIG: `new = (old & !mask) | (value & mask)`.
    fn update_config(&mut self, mask: u16, value: u16) -> Result<(), I2C::Error> {
        let config = self.read_register(REG_CONFIG)?;
        self.write_register(REG_CONFIG, (config & !mask) | (value & mask))
    }
}
