const DEFAULT_ADDRESS: u8 = 0x44;
const ALTERNATE_ADDRESS: u8 = 0x45;

const CRC_POLYNOMIAL: u8 = 0x31;
const CRC_INIT: u8 = 0xFF;
const MAX_RAW: f32 = 65_535.0;

/// Driver result type.
pub type Result<T, E> = core::result::Result<T, Error<E>>;

/// SHT3x-DIS I2C address selection.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Address(u8);

impl Address {
    /// `0x44`, selected when ADDR is tied low.
    pub const DEFAULT: Self = Self(DEFAULT_ADDRESS);

    /// `0x45`, selected when ADDR is tied high.
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

/// Measurement repeatability.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Repeatability {
    /// Shortest conversion time with the lowest repeatability.
    Low,
    /// Balanced conversion time and repeatability.
    Medium,
    /// Longest conversion time with the highest repeatability.
    High,
}

impl Repeatability {
    /// Conservative conversion delay in milliseconds for normal SHT3x supply.
    ///
    /// These values match the SHT3x-DIS datasheet Table 4 maxima rounded up:
    /// 4 ms, 6 ms, and 15 ms. Use [`Self::low_voltage_delay_ms`] below 2.4 V.
    #[must_use]
    pub const fn delay_ms(self) -> u32 {
        match self {
            Self::Low => 4,
            Self::Medium => 6,
            Self::High => 15,
        }
    }

    /// Conservative conversion delay in milliseconds for VDD below 2.4 V.
    ///
    /// These values match the SHT3x-DIS datasheet Table 5 maxima rounded up:
    /// 4.5 ms, 6.5 ms, and 15.5 ms become 5 ms, 7 ms, and 16 ms.
    #[must_use]
    pub const fn low_voltage_delay_ms(self) -> u32 {
        match self {
            Self::Low => 5,
            Self::Medium => 7,
            Self::High => 16,
        }
    }

    pub(crate) const fn single_shot_command(self, clock_stretching: bool) -> u16 {
        match (clock_stretching, self) {
            (true, Self::High) => 0x2C06,
            (true, Self::Medium) => 0x2C0D,
            (true, Self::Low) => 0x2C10,
            (false, Self::High) => 0x2400,
            (false, Self::Medium) => 0x240B,
            (false, Self::Low) => 0x2416,
        }
    }
}

/// Periodic acquisition rate.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum PeriodicRate {
    /// 0.5 measurements per second.
    Mps0_5,
    /// 1 measurement per second.
    Mps1,
    /// 2 measurements per second.
    Mps2,
    /// 4 measurements per second.
    Mps4,
    /// 10 measurements per second.
    ///
    /// At the highest periodic rate, sensor self-heating may occur and can skew
    /// temperature and relative humidity readings. The size of this offset
    /// depends on ambient airflow, enclosure design, and PCB thermal layout.
    Mps10,
}

impl PeriodicRate {
    pub(crate) const fn command(self, repeatability: Repeatability) -> u16 {
        match (self, repeatability) {
            (Self::Mps0_5, Repeatability::High) => 0x2032,
            (Self::Mps0_5, Repeatability::Medium) => 0x2024,
            (Self::Mps0_5, Repeatability::Low) => 0x202F,
            (Self::Mps1, Repeatability::High) => 0x2130,
            (Self::Mps1, Repeatability::Medium) => 0x2126,
            (Self::Mps1, Repeatability::Low) => 0x212D,
            (Self::Mps2, Repeatability::High) => 0x2236,
            (Self::Mps2, Repeatability::Medium) => 0x2220,
            (Self::Mps2, Repeatability::Low) => 0x222B,
            (Self::Mps4, Repeatability::High) => 0x2334,
            (Self::Mps4, Repeatability::Medium) => 0x2322,
            (Self::Mps4, Repeatability::Low) => 0x2329,
            (Self::Mps10, Repeatability::High) => 0x2737,
            (Self::Mps10, Repeatability::Medium) => 0x2721,
            (Self::Mps10, Repeatability::Low) => 0x272A,
        }
    }
}

/// Raw 16-bit sensor output.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct RawMeasurement {
    /// Raw temperature word from the sensor.
    pub temperature: u16,
    /// Raw relative humidity word from the sensor.
    pub humidity: u16,
}

impl RawMeasurement {
    /// Converts raw sensor output to physical units.
    #[must_use]
    pub fn to_measurement(self) -> Measurement {
        let temperature_raw = self.temperature as f32;
        let humidity_raw = self.humidity as f32;
        let relative_humidity = 100.0 * humidity_raw / MAX_RAW;

        Measurement {
            temperature_celsius: -45.0 + 175.0 * temperature_raw / MAX_RAW,
            relative_humidity: relative_humidity.clamp(0.0, 100.0),
        }
    }

    /// Converts raw sensor output using integer-only fixed-point units.
    #[must_use]
    pub const fn to_fixed_point(self) -> FixedPointMeasurement {
        FixedPointMeasurement {
            temperature_millicelsius: self.temperature_millicelsius(),
            relative_humidity_hundredths: self.relative_humidity_hundredths(),
        }
    }

    /// Converts only the raw temperature output to millidegrees Celsius.
    ///
    /// For example, `-45000` means `-45.000 deg C`.
    #[must_use]
    pub const fn temperature_millicelsius(self) -> i32 {
        temperature_millicelsius_from_raw(self.temperature)
    }

    /// Converts only the raw humidity output to hundredths of a percent RH.
    ///
    /// For example, `4512` means `45.12 %RH`.
    #[must_use]
    pub const fn relative_humidity_hundredths(self) -> u16 {
        let raw = self.humidity as u32;
        ((10_000 * raw) / 65_535) as u16
    }

    /// Converts only the raw temperature output to degrees Celsius.
    #[must_use]
    pub fn temperature_celsius(self) -> f32 {
        temperature_celsius_from_raw(self.temperature)
    }

    /// Converts only the raw humidity output to relative humidity in percent.
    #[must_use]
    pub fn relative_humidity(self) -> f32 {
        self.to_measurement().relative_humidity
    }
}

/// Converted sensor output.
#[derive(Debug, Copy, Clone, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Measurement {
    /// Temperature in degrees Celsius.
    pub temperature_celsius: f32,
    /// Relative humidity in percent RH, clamped to `0.0..=100.0`.
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

/// Status register bits.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Status(pub u16);

impl Status {
    /// `true` when the alert pin condition is currently active.
    #[must_use]
    pub const fn alert_pending(self) -> bool {
        self.0 & (1 << 15) != 0
    }

    /// `true` when the internal heater is enabled.
    #[must_use]
    pub const fn heater_enabled(self) -> bool {
        self.0 & (1 << 13) != 0
    }

    /// `true` when the humidity tracking alert condition is active.
    #[must_use]
    pub const fn humidity_alert(self) -> bool {
        self.0 & (1 << 11) != 0
    }

    /// `true` when the temperature tracking alert condition is active.
    #[must_use]
    pub const fn temperature_alert(self) -> bool {
        self.0 & (1 << 10) != 0
    }

    /// `true` when the sensor detected a reset since the flag was last cleared.
    #[must_use]
    pub const fn reset_detected(self) -> bool {
        self.0 & (1 << 4) != 0
    }

    /// `true` when the last command was not processed successfully.
    #[must_use]
    pub const fn command_failed(self) -> bool {
        self.0 & (1 << 1) != 0
    }

    /// `true` when the last write command failed its checksum validation.
    #[must_use]
    pub const fn write_checksum_failed(self) -> bool {
        self.0 & 1 != 0
    }
}

/// Driver errors.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error<I2cError> {
    /// I2C bus error from the HAL.
    I2c(I2cError),
    /// Periodic fetch was attempted before a measurement was ready.
    NotReady,
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

impl<I2cError> core::fmt::Display for Error<I2cError>
where
    I2cError: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::I2c(error) => write!(f, "I2C bus error: {error:?}"),
            Self::NotReady => f.write_str("measurement not ready"),
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

impl<I2cError> core::error::Error for Error<I2cError>
where
    I2cError: core::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::I2c(error) => Some(error),
            Self::NotReady | Self::Crc { .. } => None,
        }
    }
}

/// Data word associated with a CRC failure.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DataWord {
    /// The temperature data word.
    Temperature,
    /// The relative humidity data word.
    Humidity,
    /// The status register word.
    Status,
}

pub(crate) fn parse_raw_measurement<E>(data: [u8; 6]) -> Result<RawMeasurement, E> {
    check_crc(DataWord::Temperature, data[0], data[1], data[2])?;
    check_crc(DataWord::Humidity, data[3], data[4], data[5])?;

    Ok(RawMeasurement {
        temperature: u16::from_be_bytes([data[0], data[1]]),
        humidity: u16::from_be_bytes([data[3], data[4]]),
    })
}

pub(crate) fn map_fetch_error<I2cError>(error: Error<I2cError>) -> Error<I2cError>
where
    I2cError: embedded_hal::i2c::Error,
{
    match error {
        Error::I2c(i2c_error)
            if matches!(
                i2c_error.kind(),
                embedded_hal::i2c::ErrorKind::NoAcknowledge(
                    embedded_hal::i2c::NoAcknowledgeSource::Address
                )
            ) =>
        {
            Error::NotReady
        }
        other => other,
    }
}

pub(crate) fn parse_raw_temperature<E>(data: [u8; 3]) -> Result<u16, E> {
    check_crc(DataWord::Temperature, data[0], data[1], data[2])?;
    Ok(u16::from_be_bytes([data[0], data[1]]))
}

pub(crate) fn temperature_celsius_from_raw(raw: u16) -> f32 {
    -45.0 + 175.0 * raw as f32 / MAX_RAW
}

pub(crate) const fn temperature_millicelsius_from_raw(raw: u16) -> i32 {
    let raw = raw as i64;
    (-45_000 + (175_000 * raw) / 65_535) as i32
}

pub(crate) fn check_crc<E>(word: DataWord, msb: u8, lsb: u8, actual: u8) -> Result<(), E> {
    let expected = crc8([msb, lsb]);
    if actual == expected {
        Ok(())
    } else {
        Err(Error::Crc {
            word,
            expected,
            actual,
        })
    }
}

/// Calculates the SHT3x CRC-8 over one 16-bit data word.
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
