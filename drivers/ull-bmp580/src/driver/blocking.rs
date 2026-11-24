use embedded_hal::{
    delay::DelayNs,
    i2c::{Error as _, ErrorKind, I2c, SevenBitAddress},
};

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
    I2C: I2c<SevenBitAddress>,
{
    /// Reads CHIP_ID (`0x50` on the BMP580).
    pub fn chip_id(&mut self) -> Result<u8, I2C::Error> {
        self.read_register(REG_CHIP_ID)
    }

    /// Reads CHIP_ID and returns [`Error::UnexpectedChipId`] if it is not `0x50`.
    pub fn check_chip_id(&mut self) -> Result<(), I2C::Error> {
        match self.chip_id()? {
            CHIP_ID => Ok(()),
            other => Err(Error::UnexpectedChipId(other)),
        }
    }

    /// Soft reset: writes `0xB6` to CMD and waits the 2 ms reset time.
    ///
    /// The sensor does not acknowledge this write (p. 66), so a NACK from the
    /// bus is treated as success. All user configuration returns to defaults.
    pub fn soft_reset<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        match self.i2c.write(self.address, &[REG_CMD, CMD_SOFT_RESET]) {
            Ok(()) => {}
            Err(error) if matches!(error.kind(), ErrorKind::NoAcknowledge(_)) => {}
            Err(error) => return Err(Error::Bus(error)),
        }
        delay.delay_ms(SOFT_RESET_DELAY_MS);
        Ok(())
    }

    /// Resets the sensor and runs the post-power-up checks from section 4.3.9
    /// (p. 18): soft reset, CHIP_ID is `0x50`, STATUS shows the NVM ready
    /// without error, then INT_STATUS is read to clear the reset flag.
    ///
    /// Wait 2 ms after power-up before calling this (`t_powup`, p. 10).
    pub fn init<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.soft_reset(delay)?;
        self.check_chip_id()?;
        if !nvm_ok(self.read_register(REG_STATUS)?) {
            return Err(Error::Nvm);
        }
        self.interrupt_status()?;
        Ok(())
    }

    /// Reads the current power mode (ODR_CONFIG.pwr_mode returns the actual mode).
    pub fn power_mode(&mut self) -> Result<PowerMode, I2C::Error> {
        self.read_register(REG_ODR_CONFIG).map(PowerMode::from_bits)
    }

    /// Requests a power mode by read-modify-write of ODR_CONFIG.
    ///
    /// Modes can only be entered from standby (Figure 2, p. 17). Switching to
    /// standby takes up to 2.5 ms; use [`Self::set_standby`] to wait for it.
    pub fn set_power_mode(&mut self, mode: PowerMode) -> Result<(), I2C::Error> {
        let odr = self.read_register(REG_ODR_CONFIG)?;
        self.write_register(REG_ODR_CONFIG, (odr & !PWR_MODE_MASK) | mode as u8)
    }

    /// Switches to standby and waits the 2.5 ms transition time (rounded up to 3 ms).
    pub fn set_standby<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.set_power_mode(PowerMode::Standby)?;
        delay.delay_ms(STANDBY_DELAY_MS);
        Ok(())
    }

    /// Applies a measurement configuration and leaves the sensor in standby.
    ///
    /// Goes to standby first if needed (configuration writes are lost in other
    /// modes, p. 18), then writes DSP_IIR, OSR_CONFIG and ODR_CONFIG.
    pub fn configure<D>(&mut self, delay: &mut D, config: &Config) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        if self.power_mode()? != PowerMode::Standby {
            self.set_standby(delay)?;
        }
        self.write_register(REG_DSP_IIR, config.iir_register())?;
        self.write_register(REG_OSR_CONFIG, config.osr_register())?;
        self.write_register(REG_ODR_CONFIG, config.odr_register(PowerMode::Standby))
    }

    /// Returns `true` if the configured ODR/OSR combination is valid
    /// (OSR_EFF.odr_is_valid). Only checked by the sensor in normal mode.
    pub fn odr_osr_valid(&mut self) -> Result<bool, I2C::Error> {
        self.read_register(REG_OSR_EFF)
            .map(|value| value & OSR_EFF_ODR_VALID != 0)
    }

    /// Runs one forced measurement and returns the raw result.
    ///
    /// Reads OSR_CONFIG and ODR_CONFIG, goes to standby first if the sensor is
    /// in another mode, triggers forced mode, waits start-up plus conversion
    /// time for the configured oversampling, then burst-reads the data
    /// registers. The sensor returns to standby by itself.
    pub fn measure_forced<D>(&mut self, delay: &mut D) -> Result<RawMeasurement, I2C::Error>
    where
        D: DelayNs,
    {
        let mut regs = [0u8; 2];
        self.read_registers(REG_OSR_CONFIG, &mut regs)?;
        let [osr, odr] = regs;
        let standby = odr & !PWR_MODE_MASK;
        if odr & PWR_MODE_MASK != PowerMode::Standby as u8 {
            self.write_register(REG_ODR_CONFIG, standby)?;
            delay.delay_ms(STANDBY_DELAY_MS);
        }
        self.write_register(REG_ODR_CONFIG, standby | PowerMode::Forced as u8)?;
        delay.delay_ms(forced_measurement_delay_ms(osr));
        self.read_measurement()
    }

    /// Burst-reads temperature and pressure (six bytes from 0x1D).
    ///
    /// Use a single burst so the values come from the same measurement
    /// (data shadowing, section 4.5.1, p. 22). Pressure is only valid when
    /// pressure measurement is enabled.
    pub fn read_measurement(&mut self) -> Result<RawMeasurement, I2C::Error> {
        let mut data = [0u8; 6];
        self.read_registers(REG_DATA_START, &mut data)?;
        Ok(RawMeasurement::from_bytes(data))
    }

    /// Burst-reads only the temperature (three bytes from 0x1D). Raw value is
    /// 1/65536 degC per LSb.
    pub fn read_temperature_raw(&mut self) -> Result<i32, I2C::Error> {
        let mut data = [0u8; 3];
        self.read_registers(REG_DATA_START, &mut data)?;
        Ok(crate::types::sign_extend_24(data[0], data[1], data[2]))
    }

    /// Reads INT_STATUS. The register clears on read (p. 58); in latched mode
    /// this is also what releases the INT pin for data ready.
    pub fn interrupt_status(&mut self) -> Result<InterruptStatus, I2C::Error> {
        self.read_register(REG_INT_STATUS).map(InterruptStatus)
    }

    /// Reads INT_CONFIG as a configuration plus the pin-enable bit.
    pub fn interrupt_config(&mut self) -> Result<(InterruptConfig, bool), I2C::Error> {
        self.read_register(REG_INT_CONFIG)
            .map(InterruptConfig::from_register)
    }

    /// Writes INT_CONFIG only. Switching between latched and pulsed while
    /// interrupts are enabled is not allowed (p. 29); use
    /// [`Self::configure_data_ready_interrupt`] for a safe sequence.
    ///
    /// Returns [`Error::InvalidConfig`] for an invalid drive strength.
    pub fn set_interrupt_config(
        &mut self,
        config: &InterruptConfig,
        enabled: bool,
    ) -> Result<(), I2C::Error> {
        let value = config.to_register(enabled).ok_or(Error::InvalidConfig)?;
        self.write_register(REG_INT_CONFIG, value)
    }

    /// Enables the data-ready interrupt on the INT pin with `config`.
    ///
    /// Follows the mode-switch sequence from p. 29: INT_SOURCE = 0, read
    /// INT_STATUS to clear it, write INT_CONFIG (pin enabled), then set
    /// INT_SOURCE.drdy_data_reg_en. Pass [`InterruptConfig::default`] to keep
    /// the reset pin behaviour (latched, active low, open drain); nothing is
    /// changed to push-pull unless `config.output` says so.
    pub fn configure_data_ready_interrupt(
        &mut self,
        config: &InterruptConfig,
    ) -> Result<(), I2C::Error> {
        let value = config.to_register(true).ok_or(Error::InvalidConfig)?;
        self.write_register(REG_INT_SOURCE, 0x00)?;
        self.interrupt_status()?;
        self.write_register(REG_INT_CONFIG, value)?;
        self.write_register(REG_INT_SOURCE, INT_SOURCE_DRDY)
    }

    /// Disables all interrupt sources (INT_SOURCE = 0). The INT_CONFIG pin
    /// settings are left unchanged.
    pub fn disable_interrupts(&mut self) -> Result<(), I2C::Error> {
        self.write_register(REG_INT_SOURCE, 0x00)
    }

    /// Raw escape hatch: reads one register.
    pub fn read_register(&mut self, register: u8) -> Result<u8, I2C::Error> {
        let mut data = [0u8; 1];
        self.read_registers(register, &mut data)?;
        Ok(data[0])
    }

    /// Raw escape hatch: burst-reads registers starting at `register`
    /// (address auto-increments, section 5.6, p. 38-41).
    pub fn read_registers(&mut self, register: u8, buffer: &mut [u8]) -> Result<(), I2C::Error> {
        self.i2c
            .write_read(self.address, &[register], buffer)
            .map_err(Error::Bus)
    }

    /// Raw escape hatch: writes one register.
    pub fn write_register(&mut self, register: u8, value: u8) -> Result<(), I2C::Error> {
        self.i2c
            .write(self.address, &[register, value])
            .map_err(Error::Bus)
    }
}
