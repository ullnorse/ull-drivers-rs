mod blocking;

#[cfg(feature = "async")]
mod r#async;

use crate::types::Address;

// Command bytes: Table 7 (p. 10). Measurement commands are in `Repeatability`,
// heater commands in `types::heater_command`.
const CMD_READ_SERIAL: u8 = 0x89;
const CMD_SOFT_RESET: u8 = 0x94;

/// Soft reset time, Table 4 (p. 8): 1 ms max.
const SOFT_RESET_DELAY_MS: u32 = 1;

/// Sensirion SHT4x (SHT40) embedded-hal 1.0 I2C driver.
///
/// Every read is CRC-checked. Measurements are single-shot: the driver writes
/// the command, waits the datasheet conversion time, then reads six bytes.
///
/// The sensor does not support clock stretching and NACKs the read header while
/// busy (section 4.1, p. 8-9), so the delay is mandatory.
#[derive(Debug)]
pub struct Sht4x<I2C> {
    i2c: I2C,
    address: u8,
}

impl<I2C> Sht4x<I2C> {
    /// Creates a driver using the default `0x44` address.
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

impl<I2C> From<I2C> for Sht4x<I2C> {
    fn from(i2c: I2C) -> Self {
        Self::new(i2c)
    }
}
