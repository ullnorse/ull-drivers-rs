// Datasheet: Sensirion SHT4x, Version 2 - July 2021 (SHT40.pdf). Page numbers
// below are PDF pages.

/// SHT40-AD1B ordering option, Table 10 / page 1 device overview (p. 15).
const DEFAULT_ADDRESS: u8 = 0x44;
/// SHT40-BD1B ordering option, Table 10 (p. 15).
const ALTERNATE_ADDRESS: u8 = 0x45;

/// CRC-8 polynomial 0x31, Table 6 (p. 9).
const CRC_POLYNOMIAL: u8 = 0x31;
/// CRC-8 initialisation 0xFF, Table 6 (p. 9). No reflection, final XOR 0x00.
const CRC_INIT: u8 = 0xFF;
/// 2^16 - 1, the denominator in equations (1) and (2), section 4.5 (p. 10).
const MAX_RAW: u32 = 65_535;

/// Driver result type.
pub type Result<T, E> = core::result::Result<T, Error<E>>;

/// SHT4x I2C address selection.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Address(u8);

impl Address {
    /// `0x44`, the SHT40-AD1B address.
    pub const DEFAULT: Self = Self(DEFAULT_ADDRESS);

    /// `0x45`, the SHT40-BD1B address.
    pub const ALTERNATE: Self = Self(ALTERNATE_ADDRESS);

    /// Creates an address from a supported 7-bit sensor address.
    #[must_use]
    pub const fn custom(address: u8) -> Option<Self> {
        if address == DEFAULT_ADDRESS || address == ALTERNATE_ADDRESS {
            Some(Self(address))
        } else {
            None
        }
    }

    /// Returns the 7-bit I2C address.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self.0
    }
}

impl Default for Address {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Measurement repeatability (precision) selection.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Repeatability {
    /// Lowest precision, shortest conversion.
    Low,
    /// Medium precision.
    Medium,
    /// Highest precision, longest conversion.
    High,
}

impl Repeatability {
    /// Conversion delay in milliseconds.
    ///
    /// Table 4 (p. 8) maxima are 1.7 ms (low), 4.5 ms (medium) and 8.2 ms
    /// (high); these are rounded up to whole milliseconds.
    #[must_use]
    pub const fn delay_ms(self) -> u32 {
        match self {
            Self::Low => 2,
            Self::Medium => 5,
            Self::High => 9,
        }
    }

    /// Measurement command byte, Table 7 (p. 10).
    pub(crate) const fn command(self) -> u8 {
        match self {
            Self::High => 0xFD,
            Self::Medium => 0xF6,
            Self::Low => 0xE0,
        }
    }
}

/// Heater power for the raw heater commands (Table 7, p. 10; typical at 3.3 V).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum HeaterPower {
    /// 20 mW.
    Mw20,
    /// 110 mW.
    Mw110,
    /// 200 mW.
    Mw200,
}

/// Heater pulse length for the raw heater commands (Table 7, p. 10).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum HeaterDuration {
    /// 0.1 s pulse (Table 4: 0.09 to 0.11 s).
    Short,
    /// 1 s pulse (Table 4: 0.9 to 1.1 s).
    Long,
}

impl HeaterDuration {
    /// Time to wait before reading: the maximum heater-on time from Table 4
    /// (p. 8) plus the 8.2 ms high-repeatability measurement, rounded up.
    #[must_use]
    pub const fn delay_ms(self) -> u32 {
        match self {
            Self::Short => 110 + 9,
            Self::Long => 1_100 + 9,
        }
    }
}

/// Heater command byte, Table 7 (p. 10).
pub(crate) const fn heater_command(power: HeaterPower, duration: HeaterDuration) -> u8 {
    match (power, duration) {
        (HeaterPower::Mw200, HeaterDuration::Long) => 0x39,
        (HeaterPower::Mw200, HeaterDuration::Short) => 0x32,
        (HeaterPower::Mw110, HeaterDuration::Long) => 0x2F,
        (HeaterPower::Mw110, HeaterDuration::Short) => 0x24,
        (HeaterPower::Mw20, HeaterDuration::Long) => 0x1E,
        (HeaterPower::Mw20, HeaterDuration::Short) => 0x15,
    }
}

/// Raw 16-bit sensor output (ticks).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct RawMeasurement {
    /// Raw temperature ticks.
    pub temperature: u16,
    /// Raw relative humidity ticks.
    pub humidity: u16,
}

impl RawMeasurement {
    /// Converts raw ticks to physical units as `f32`.
    #[must_use]
    pub fn to_measurement(self) -> Measurement {
        Measurement {
            temperature_celsius: self.temperature_celsius(),
            relative_humidity: self.relative_humidity(),
        }
    }

    /// Converts raw ticks using integer-only fixed-point units.
    #[must_use]
    pub const fn to_fixed_point(self) -> FixedPointMeasurement {
        FixedPointMeasurement {
            temperature_millicelsius: self.temperature_millicelsius(),
            relative_humidity_hundredths: self.relative_humidity_hundredths(),
        }
    }

    /// Temperature in millidegrees Celsius, equation (2) (p. 10):
    /// `T = -45 + 175 * ticks / 65535`. Fraction truncated.
    #[must_use]
    pub const fn temperature_millicelsius(self) -> i32 {
        (-45_000 + (175_000 * self.temperature as i64) / MAX_RAW as i64) as i32
    }

    /// Relative humidity in hundredths of a percent, equation (1) (p. 10):
    /// `RH = -6 + 125 * ticks / 65535`, cropped to 0..=100 %RH as the datasheet
    /// advises. Fraction truncated.
    #[must_use]
    pub const fn relative_humidity_hundredths(self) -> u16 {
        let value = -600 + (12_500 * self.humidity as i64) / MAX_RAW as i64;
        if value < 0 {
            0
        } else if value > 10_000 {
            10_000
        } else {
            value as u16
        }
    }

    /// Temperature in degrees Celsius.
    #[must_use]
    pub fn temperature_celsius(self) -> f32 {
        -45.0 + 175.0 * self.temperature as f32 / MAX_RAW as f32
    }

    /// Relative humidity in percent, cropped to `0.0..=100.0`.
    #[must_use]
    pub fn relative_humidity(self) -> f32 {
        (-6.0 + 125.0 * self.humidity as f32 / MAX_RAW as f32).clamp(0.0, 100.0)
    }
}

/// Converted sensor output.
#[derive(Debug, Copy, Clone, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Measurement {
    /// Temperature in degrees Celsius.
    pub temperature_celsius: f32,
    /// Relative humidity in percent RH, cropped to `0.0..=100.0`.
    pub relative_humidity: f32,
}

/// Integer-only converted sensor output.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct FixedPointMeasurement {
    /// Temperature in millidegrees Celsius. `21562` means `21.562 deg C`.
    pub temperature_millicelsius: i32,
    /// Relative humidity in hundredths of a percent. `4512` means `45.12 %RH`.
    pub relative_humidity_hundredths: u16,
}

/// Data word associated with a CRC failure.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DataWord {
    /// The temperature data word.
    Temperature,
    /// The relative humidity data word.
    Humidity,
    /// First (upper) serial number word.
    SerialHigh,
    /// Second (lower) serial number word.
    SerialLow,
}

/// Driver errors.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error<BusError> {
    /// Bus error from the HAL.
    Bus(BusError),
    /// CRC byte did not match the preceding data word.
    Crc {
        /// Which 16-bit word failed CRC validation.
        word: DataWord,
        /// CRC value computed from the received data bytes.
        expected: u8,
        /// CRC byte returned by the sensor.
        actual: u8,
    },
}

impl<BusError> core::fmt::Display for Error<BusError>
where
    BusError: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bus(error) => write!(f, "sensor bus error: {error:?}"),
            Self::Crc {
                word,
                expected,
                actual,
            } => write!(
                f,
                "CRC mismatch for {word:?}: expected 0x{expected:02X}, got 0x{actual:02X}"
            ),
        }
    }
}

impl<BusError> core::error::Error for Error<BusError>
where
    BusError: core::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Bus(error) => Some(error),
            Self::Crc { .. } => None,
        }
    }
}

pub(crate) fn check_crc<E>(word: DataWord, bytes: [u8; 3]) -> Result<u16, E> {
    let expected = crc8([bytes[0], bytes[1]]);
    if bytes[2] == expected {
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    } else {
        Err(Error::Crc {
            word,
            expected,
            actual: bytes[2],
        })
    }
}

pub(crate) fn parse_raw_measurement<E>(data: [u8; 6]) -> Result<RawMeasurement, E> {
    Ok(RawMeasurement {
        temperature: check_crc(DataWord::Temperature, [data[0], data[1], data[2]])?,
        humidity: check_crc(DataWord::Humidity, [data[3], data[4], data[5]])?,
    })
}

pub(crate) fn parse_serial_number<E>(data: [u8; 6]) -> Result<u32, E> {
    let high = check_crc(DataWord::SerialHigh, [data[0], data[1], data[2]])?;
    let low = check_crc(DataWord::SerialLow, [data[3], data[4], data[5]])?;
    Ok((u32::from(high) << 16) | u32::from(low))
}

/// Calculates the SHT4x CRC-8 over one 16-bit data word (Table 6, p. 9:
/// polynomial 0x31, init 0xFF, no reflection, final XOR 0x00).
#[must_use]
pub fn crc8(data: [u8; 2]) -> u8 {
    let mut crc = CRC_INIT;

    for byte in data {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ CRC_POLYNOMIAL
            } else {
                crc << 1
            };
        }
    }

    crc
}
