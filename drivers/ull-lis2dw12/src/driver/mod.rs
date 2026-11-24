mod blocking;

#[cfg(feature = "async")]
mod r#async;

use crate::types::Address;

// Register addresses, register map Table 22 (pp. 40-41).
const REG_OUT_T: u8 = 0x26;
const REG_WHO_AM_I: u8 = 0x0F;
const REG_CTRL1: u8 = 0x20;
const REG_CTRL2: u8 = 0x21;
const REG_CTRL3: u8 = 0x22;
const REG_CTRL4_INT1_PAD: u8 = 0x23;
const REG_CTRL6: u8 = 0x25;
const REG_STATUS: u8 = 0x27;
const REG_OUT_X_L: u8 = 0x28; // burst of 6 bytes to 0x2D
const REG_WAKE_UP_THS: u8 = 0x34;
const REG_WAKE_UP_DUR: u8 = 0x35;
const REG_WAKE_UP_SRC: u8 = 0x38;
const REG_ALL_INT_SRC: u8 = 0x3B;
const REG_CTRL7: u8 = 0x3F;

/// CTRL2 (0x21) bits, section 8.5 (p. 44): SOFT_RESET bit 6, BDU bit 3,
/// IF_ADD_INC bit 2 (reset value 1, Table 22 gives CTRL2 reset 0x04).
const CTRL2_SOFT_RESET: u8 = 1 << 6;
const CTRL2_BDU: u8 = 1 << 3;
const CTRL2_IF_ADD_INC: u8 = 1 << 2;

/// CTRL3 (0x22) bits, section 8.6 (p. 45): PP_OD bit 5, LIR bit 4, H_LACTIVE
/// bit 3, SLP_MODE_SEL bit 1, SLP_MODE_1 bit 0.
const CTRL3_PIN_MASK: u8 = 0b0011_1000;
const CTRL3_SLP_MODE_SEL: u8 = 1 << 1;
const CTRL3_SLP_MODE_1: u8 = 1 << 0;

/// CTRL4_INT1_PAD_CTRL (0x23) INT1_WU bit, section 8.7 (p. 46).
const CTRL4_INT1_WU: u8 = 1 << 5;

/// CTRL7 (0x3F) INTERRUPTS_ENABLE bit, section 8.35 (p. 60).
const CTRL7_INTERRUPTS_ENABLE: u8 = 1 << 5;

/// ODR[3:0] mask in CTRL1, section 8.4 (p. 43).
const CTRL1_ODR_MASK: u8 = 0xF0;

/// The datasheet does not give a soft-reset duration. SOFT_RESET is polled
/// (it "goes to 0" when done, p. 44) once per millisecond, up to this many times.
const SOFT_RESET_POLL_MS: u32 = 1;
const SOFT_RESET_POLL_ATTEMPTS: u32 = 10;

/// ST LIS2DW12 embedded-hal 1.0 I2C driver.
///
/// Typical flow: [`Self::init`], [`Self::configure`], then
/// [`Self::read_acceleration_mg`]. For deep-sleep wake on INT1 call
/// [`Self::set_interrupt_pin_config`] and [`Self::enable_wake_up_interrupt`].
///
/// CS must be tied high on the board to select I2C (p. 22).
#[derive(Debug)]
pub struct Lis2dw12<I2C> {
    i2c: I2C,
    address: u8,
}

impl<I2C> Lis2dw12<I2C> {
    /// Creates a driver using the default `0x19` address (SA0 high).
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

impl<I2C> From<I2C> for Lis2dw12<I2C> {
    fn from(i2c: I2C) -> Self {
        Self::new(i2c)
    }
}
