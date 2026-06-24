mod blocking;

#[cfg(feature = "async")]
mod asynchronous;

use core::marker::PhantomData;

use crate::types::{Address, DataWord, Result, Status, check_crc};

const GENERAL_CALL_ADDRESS: u8 = 0x00;

const CMD_FETCH_DATA: u16 = 0xE000;
const CMD_ART: u16 = 0x2B32;
const CMD_BREAK: u16 = 0x3093;
const CMD_SOFT_RESET: u16 = 0x30A2;
const CMD_HEATER_ENABLE: u16 = 0x306D;
const CMD_HEATER_DISABLE: u16 = 0x3066;
const CMD_READ_STATUS: u16 = 0xF32D;
const CMD_CLEAR_STATUS: u16 = 0x3041;

const COMMAND_DELAY_MS: u32 = 1;

/// Marker type for the default single-shot command mode.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct SingleShotMode;

/// Marker type for periodic acquisition mode.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct PeriodicMode;

/// Marker type for ART acquisition mode.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct ArtMode;

/// Sensirion SHT3x-DIS embedded-hal 1.0 I2C driver.
///
/// Most applications should start with [`Self::measure`] for one-shot readings
/// or [`Self::start_periodic_and_wait`] followed by `fetch` on the returned
/// periodic-mode driver.
///
/// Use the more specialized methods only when you need a specific tradeoff:
///
/// - [`Self::measure_raw`] if you want raw `u16` words or integer-only
///   conversion via [`crate::RawMeasurement::to_fixed_point`].
/// - [`Self::measure_temperature`] or [`Self::measure_temperature_millicelsius`]
///   if humidity is not needed and you want a shorter read transaction.
/// - `*_low_voltage` variants when VDD is below 2.4 V and the longer datasheet
///   conversion delays must be used.
/// - `*_with_clock_stretching` variants only when the I2C controller supports
///   sensor-driven clock stretching.
/// - `_and_wait` configuration methods when you want the driver to enforce the
///   datasheet's required 1 ms command gap.
#[derive(Debug)]
pub struct Sht3x<I2C, MODE = SingleShotMode> {
    i2c: I2C,
    address: u8,
    _mode: PhantomData<MODE>,
}

impl<I2C> Sht3x<I2C> {
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
            _mode: PhantomData,
        }
    }
}

impl<I2C, MODE> Sht3x<I2C, MODE> {
    fn into_mode<NEXT>(self) -> Sht3x<I2C, NEXT> {
        Sht3x {
            i2c: self.i2c,
            address: self.address,
            _mode: PhantomData,
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

impl<I2C> From<I2C> for Sht3x<I2C> {
    fn from(i2c: I2C) -> Self {
        Self::new(i2c)
    }
}

const fn heater_command(enabled: bool) -> u16 {
    if enabled {
        CMD_HEATER_ENABLE
    } else {
        CMD_HEATER_DISABLE
    }
}

fn parse_status<E>(data: [u8; 3]) -> Result<Status, E> {
    check_crc(DataWord::Status, data[0], data[1], data[2])?;
    Ok(Status(u16::from_be_bytes([data[0], data[1]])))
}
