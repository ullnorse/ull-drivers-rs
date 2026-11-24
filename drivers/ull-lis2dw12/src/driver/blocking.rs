use embedded_hal::{
    delay::DelayNs,
    i2c::{I2c, SevenBitAddress},
};

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
    I2C: I2c<SevenBitAddress>,
{
    /// Reads WHO_AM_I (`0x44` on the LIS2DW12).
    pub fn who_am_i(&mut self) -> Result<u8, I2C::Error> {
        self.read_register(REG_WHO_AM_I)
    }

    /// Reads WHO_AM_I and returns [`Error::UnexpectedChipId`] if it is not `0x44`.
    pub fn check_who_am_i(&mut self) -> Result<(), I2C::Error> {
        match self.who_am_i()? {
            WHO_AM_I => Ok(()),
            other => Err(Error::UnexpectedChipId(other)),
        }
    }

    /// Soft reset: sets CTRL2.SOFT_RESET (keeping IF_ADD_INC set) and polls
    /// CTRL2 once per millisecond until the bit clears.
    ///
    /// The datasheet gives no reset duration, only that the bit "goes to 0"
    /// (p. 44). All control registers return to their reset values. Returns
    /// [`Error::ResetTimeout`] after 10 polls.
    pub fn soft_reset<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_register(REG_CTRL2, CTRL2_SOFT_RESET | CTRL2_IF_ADD_INC)?;
        for _ in 0..SOFT_RESET_POLL_ATTEMPTS {
            delay.delay_ms(SOFT_RESET_POLL_MS);
            if self.read_register(REG_CTRL2)? & CTRL2_SOFT_RESET == 0 {
                return Ok(());
            }
        }
        Err(Error::ResetTimeout)
    }

    /// Soft-resets the device and checks WHO_AM_I.
    pub fn init<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.soft_reset(delay)?;
        self.check_who_am_i()
    }

    /// Writes CTRL6 (full scale, bandwidth, low noise) and then CTRL1 (ODR and
    /// mode), so the new range is in place before the data rate starts.
    pub fn configure(&mut self, config: &Config) -> Result<(), I2C::Error> {
        self.write_register(REG_CTRL6, config.ctrl6())?;
        self.write_register(REG_CTRL1, config.ctrl1())
    }

    /// Reads CTRL1 and CTRL6 back as a [`Config`].
    ///
    /// Returns [`Error::InvalidConfig`] if the registers hold the reserved
    /// MODE `0b11` or an undefined ODR code.
    pub fn read_config(&mut self) -> Result<Config, I2C::Error> {
        let ctrl1 = self.read_register(REG_CTRL1)?;
        let ctrl6 = self.read_register(REG_CTRL6)?;
        Config::from_registers(ctrl1, ctrl6).ok_or(Error::InvalidConfig)
    }

    /// Enters power-down (ODR = 0000) by read-modify-write of CTRL1.
    pub fn power_down(&mut self) -> Result<(), I2C::Error> {
        let ctrl1 = self.read_register(REG_CTRL1)?;
        self.write_register(REG_CTRL1, ctrl1 & !CTRL1_ODR_MASK)
    }

    /// Sets or clears CTRL2.BDU (block data update) by read-modify-write.
    pub fn set_block_data_update(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        self.update_register(REG_CTRL2, CTRL2_BDU, if enabled { CTRL2_BDU } else { 0 })
    }

    /// Starts one conversion in single-conversion-on-demand mode: sets
    /// CTRL3.SLP_MODE_SEL and SLP_MODE_1. Data is ready after
    /// [`crate::LowPowerMode::single_conversion_time_us`]; SLP_MODE_1 clears
    /// itself (p. 45).
    pub fn trigger_single_conversion(&mut self) -> Result<(), I2C::Error> {
        let bits = CTRL3_SLP_MODE_SEL | CTRL3_SLP_MODE_1;
        self.update_register(REG_CTRL3, bits, bits)
    }

    /// Reads STATUS.
    pub fn status(&mut self) -> Result<Status, I2C::Error> {
        self.read_register(REG_STATUS).map(Status)
    }

    /// Burst-reads X, Y and Z (six bytes from 0x28) as raw left-justified words.
    ///
    /// Relies on CTRL2.IF_ADD_INC, which is set after reset.
    pub fn read_acceleration(&mut self) -> Result<RawAcceleration, I2C::Error> {
        let mut data = [0u8; 6];
        self.read_registers(REG_OUT_X_L, &mut data)?;
        Ok(RawAcceleration::from_bytes(data))
    }

    /// Reads acceleration in mg using the full scale and resolution of `config`.
    pub fn read_acceleration_mg(&mut self, config: &Config) -> Result<[f32; 3], I2C::Error> {
        self.read_acceleration()
            .map(|raw| raw.to_milli_g(config.full_scale, config.resolution()))
    }

    /// Reads the 8-bit temperature output (OUT_T, 0x26), raw: 1 degC per LSb,
    /// 0 is 25 degC (p. 48).
    pub fn read_temperature_raw(&mut self) -> Result<i8, I2C::Error> {
        self.read_register(REG_OUT_T).map(|value| value as i8)
    }

    /// Reads the temperature in whole degrees Celsius (`25 + OUT_T`).
    pub fn read_temperature_celsius(&mut self) -> Result<i16, I2C::Error> {
        self.read_temperature_raw().map(|raw| 25 + i16::from(raw))
    }

    /// Reads the interrupt pad settings from CTRL3.
    pub fn interrupt_pin_config(&mut self) -> Result<InterruptPinConfig, I2C::Error> {
        self.read_register(REG_CTRL3)
            .map(InterruptPinConfig::from_ctrl3)
    }

    /// Sets polarity, output type and latching (CTRL3 PP_OD, LIR, H_LACTIVE)
    /// by read-modify-write, leaving self-test and on-demand bits alone.
    pub fn set_interrupt_pin_config(
        &mut self,
        config: &InterruptPinConfig,
    ) -> Result<(), I2C::Error> {
        self.update_register(REG_CTRL3, CTRL3_PIN_MASK, config.ctrl3_bits())
    }

    /// Enables the wake-up interrupt on INT1.
    ///
    /// Writes WAKE_UP_THS and WAKE_UP_DUR (other bits in both are written as
    /// 0), sets CTRL4_INT1_PAD_CTRL.INT1_WU and CTRL7.INTERRUPTS_ENABLE by
    /// read-modify-write, then reads ALL_INT_SRC to clear any stale flag so
    /// INT1 starts deasserted. The device must be running (ODR above
    /// power-down). Call [`Self::set_interrupt_pin_config`] first to choose
    /// polarity; the reset pin is active high.
    ///
    /// Returns [`Error::InvalidConfig`] without touching the bus if the
    /// threshold is above 63 or the duration above 3.
    pub fn enable_wake_up_interrupt(&mut self, config: &WakeUpConfig) -> Result<(), I2C::Error> {
        let (threshold, duration) = config.registers().ok_or(Error::InvalidConfig)?;
        self.write_register(REG_WAKE_UP_THS, threshold)?;
        self.write_register(REG_WAKE_UP_DUR, duration)?;
        self.update_register(REG_CTRL4_INT1_PAD, CTRL4_INT1_WU, CTRL4_INT1_WU)?;
        self.update_register(REG_CTRL7, CTRL7_INTERRUPTS_ENABLE, CTRL7_INTERRUPTS_ENABLE)?;
        self.clear_interrupts()?;
        Ok(())
    }

    /// Stops routing wake-up to INT1 (clears CTRL4_INT1_PAD_CTRL.INT1_WU).
    pub fn disable_wake_up_interrupt(&mut self) -> Result<(), I2C::Error> {
        self.update_register(REG_CTRL4_INT1_PAD, CTRL4_INT1_WU, 0)
    }

    /// Reads WAKE_UP_SRC: which axis crossed the threshold.
    pub fn read_wake_up_source(&mut self) -> Result<WakeUpSource, I2C::Error> {
        self.read_register(REG_WAKE_UP_SRC).map(WakeUpSource)
    }

    /// Reads ALL_INT_SRC. This resets all interrupt flags routed to the INT
    /// pads (p. 58), which releases INT1 in latched mode.
    pub fn clear_interrupts(&mut self) -> Result<AllInterruptSource, I2C::Error> {
        self.read_register(REG_ALL_INT_SRC).map(AllInterruptSource)
    }

    /// Read-modify-write: `register = (register & !mask) | (value & mask)`.
    fn update_register(&mut self, register: u8, mask: u8, value: u8) -> Result<(), I2C::Error> {
        let current = self.read_register(register)?;
        self.write_register(register, (current & !mask) | (value & mask))
    }

    /// Raw escape hatch: reads one register.
    pub fn read_register(&mut self, register: u8) -> Result<u8, I2C::Error> {
        let mut data = [0u8; 1];
        self.read_registers(register, &mut data)?;
        Ok(data[0])
    }

    /// Raw escape hatch: burst-reads registers starting at `register`
    /// (needs CTRL2.IF_ADD_INC, set after reset, section 6.1.1, p. 34).
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
