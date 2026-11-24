mod blocking;

#[cfg(feature = "async")]
mod r#async;

use crate::types::Address;

// Register addresses, register map (pp. 50-51, sections 7.1-7.36).
const REG_CHIP_ID: u8 = 0x01;
const REG_INT_CONFIG: u8 = 0x14;
const REG_INT_SOURCE: u8 = 0x15;
const REG_DATA_START: u8 = 0x1D; // TEMP_DATA_XLSB; burst of 6 bytes to 0x22
const REG_INT_STATUS: u8 = 0x27;
const REG_STATUS: u8 = 0x28;
const REG_DSP_IIR: u8 = 0x31;
const REG_OSR_CONFIG: u8 = 0x36;
const REG_ODR_CONFIG: u8 = 0x37;
const REG_OSR_EFF: u8 = 0x38;
const REG_CMD: u8 = 0x7E;

/// Soft reset command written to CMD, section 4.3.10 (p. 18) and 7.36 (p. 66).
/// The sensor does not ACK this write.
const CMD_SOFT_RESET: u8 = 0xB6;
/// INT_SOURCE.drdy_data_reg_en, section 7.6 (p. 54).
const INT_SOURCE_DRDY: u8 = 1 << 0;
/// OSR_EFF.odr_is_valid, section 7.35 (p. 65).
const OSR_EFF_ODR_VALID: u8 = 1 << 7;

/// Soft reset duration `tsoft_res` is 2 ms max (p. 11).
const SOFT_RESET_DELAY_MS: u32 = 2;
/// Time to standby `tstandby` is 2.5 ms max (p. 11), rounded up.
const STANDBY_DELAY_MS: u32 = 3;

/// Bosch BMP580 embedded-hal 1.0 I2C driver.
///
/// Typical flow: [`Self::init`], [`Self::configure`], then either
/// [`Self::measure_forced`] for low-rate sampling or
/// [`Self::set_power_mode`] with [`crate::PowerMode::Normal`] and
/// [`Self::read_measurement`].
///
/// Register writes to the configuration registers are only accepted in standby
/// (section 4.3.8, p. 18). [`Self::configure`] takes care of that; the raw
/// register methods do not.
#[derive(Debug)]
pub struct Bmp580<I2C> {
    i2c: I2C,
    address: u8,
}

impl<I2C> Bmp580<I2C> {
    /// Creates a driver using the default `0x46` address (SDO to GND).
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

impl<I2C> From<I2C> for Bmp580<I2C> {
    fn from(i2c: I2C) -> Self {
        Self::new(i2c)
    }
}
