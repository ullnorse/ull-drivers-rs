mod blocking;

#[cfg(feature = "async")]
mod r#async;

use crate::types::Address;

// Register map, Table 2 (p. 10-11). All registers are 16-bit, MSB at the even
// address, and must be read and written as whole words (p. 10, 17-18).
const REG_VCELL: u8 = 0x02;
const REG_SOC: u8 = 0x04;
const REG_MODE: u8 = 0x06;
const REG_VERSION: u8 = 0x08;
const REG_HIBRT: u8 = 0x0A;
const REG_CONFIG: u8 = 0x0C;
const REG_VALRT: u8 = 0x14;
const REG_CRATE: u8 = 0x16;
const REG_STATUS: u8 = 0x1A;

/// MODE bits as 16-bit word, Figure 8 (p. 11): MSB = X, Quick-Start, EnSleep,
/// HibStat, X, X, X, X.
const MODE_ENSLEEP: u16 = 1 << 13;
const MODE_HIBSTAT: u16 = 1 << 12;

/// HIBRT values, p. 12: 0x0000 disables hibernate, 0xFFFF always hibernates.
/// 0x8030 is the power-on value (Table 2, p. 10).
const HIBRT_DISABLE: u16 = 0x0000;
const HIBRT_ALWAYS: u16 = 0xFFFF;
const HIBRT_DEFAULT: u16 = 0x8030;

/// Maxim MAX17048 single-cell fuel gauge, embedded-hal 1.0 I2C driver.
///
/// The driver never delays, so it needs no timer. VCELL updates every 250 ms
/// in active mode and every 45 s in hibernate mode; the first SOC value is
/// available about 1 s after power-up (p. 11).
///
/// A raw register escape hatch is available through
/// [`Self::read_register`] and [`Self::write_register`].
#[derive(Debug)]
pub struct Max17048<I2C> {
    i2c: I2C,
    address: u8,
}

impl<I2C> Max17048<I2C> {
    /// Creates a driver using the fixed `0x36` address.
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

impl<I2C> From<I2C> for Max17048<I2C> {
    fn from(i2c: I2C) -> Self {
        Self::new(i2c)
    }
}
