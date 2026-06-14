#![no_std]
#![doc = include_str!("../README.md")]

use embedded_hal::{
    delay::DelayNs,
    i2c::{I2c, SevenBitAddress},
};

const DEFAULT_ADDRESS: u8 = 0x44;
const ALTERNATE_ADDRESS: u8 = 0x45;
const GENERAL_CALL_ADDRESS: u8 = 0x00;

const CMD_FETCH_DATA: u16 = 0xE000;
const CMD_ART: u16 = 0x2B32;
const CMD_BREAK: u16 = 0x3093;
const CMD_SOFT_RESET: u16 = 0x30A2;
const CMD_HEATER_ENABLE: u16 = 0x306D;
const CMD_HEATER_DISABLE: u16 = 0x3066;
const CMD_READ_STATUS: u16 = 0xF32D;
const CMD_CLEAR_STATUS: u16 = 0x3041;

const CRC_POLYNOMIAL: u8 = 0x31;
const CRC_INIT: u8 = 0xFF;
const COMMAND_DELAY_MS: u32 = 1;
const MAX_RAW: f32 = 65_535.0;

/// Driver result type.
pub type Result<T, E> = core::result::Result<T, Error<E>>;

/// SHT3x-DIS I2C address selection.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

    const fn single_shot_command(self, clock_stretching: bool) -> u16 {
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    const fn command(self, repeatability: Repeatability) -> u16 {
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

    /// Converts only the raw temperature output to millidegrees Fahrenheit.
    ///
    /// For example, `-49000` means `-49.000 deg F`.
    #[must_use]
    pub const fn temperature_millifahrenheit(self) -> i32 {
        temperature_millifahrenheit_from_raw(self.temperature)
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

    /// Converts only the raw temperature output to degrees Fahrenheit.
    #[must_use]
    pub fn temperature_fahrenheit(self) -> f32 {
        temperature_fahrenheit_from_raw(self.temperature)
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Measurement {
    /// Temperature in degrees Celsius.
    pub temperature_celsius: f32,
    /// Relative humidity in percent RH, clamped to `0.0..=100.0`.
    pub relative_humidity: f32,
}

/// Integer-only converted sensor output.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FixedPointMeasurement {
    /// Temperature in millidegrees Celsius. `21562` means `21.562 deg C`.
    pub temperature_millicelsius: i32,
    /// Relative humidity in hundredths of a percent. `4512` means `45.12 %RH`.
    pub relative_humidity_hundredths: u16,
}

/// Status register bits.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Error<I2cError> {
    /// I2C bus error from the HAL.
    I2c(I2cError),
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

/// Data word associated with a CRC failure.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DataWord {
    /// The temperature data word.
    Temperature,
    /// The relative humidity data word.
    Humidity,
    /// The status register word.
    Status,
}

/// Sensirion SHT3x-DIS embedded-hal 1.0 I2C driver.
///
/// Most applications should start with [`Self::measure`] for one-shot readings
/// or [`Self::start_periodic_and_wait`] plus [`Self::fetch`] for periodic
/// acquisition.
///
/// Use the more specialized methods only when you need a specific tradeoff:
///
/// - [`Self::measure_raw`] if you want raw `u16` words or integer-only
///   conversion via [`RawMeasurement::to_fixed_point`].
/// - [`Self::measure_temperature`] or [`Self::measure_temperature_millicelsius`]
///   if humidity is not needed and you want a shorter read transaction.
/// - `*_low_voltage` variants when VDD is below 2.4 V and the longer datasheet
///   conversion delays must be used.
/// - `*_with_clock_stretching` variants only when the I2C controller supports
///   sensor-driven clock stretching.
/// - `_and_wait` configuration methods when you want the driver to enforce the
///   datasheet's required 1 ms command gap.
#[derive(Debug)]
pub struct Sht3x<I2C> {
    i2c: I2C,
    address: u8,
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

impl<I2C> Sht3x<I2C>
where
    I2C: I2c<SevenBitAddress>,
{
    /// Triggers one measurement without clock stretching and waits for it.
    pub fn measure<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<Measurement, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_raw(delay, repeatability)
            .map(RawMeasurement::to_measurement)
    }

    /// Triggers one measurement without clock stretching and waits for it.
    pub fn measure_raw<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_raw_after_delay(delay, repeatability, repeatability.delay_ms())
    }

    /// Triggers one measurement and reads only the temperature word.
    ///
    /// The read transfer requests only the first three bytes
    /// `(temperature MSB, temperature LSB, CRC)`, aborting before humidity to
    /// reduce I2C bus traffic.
    pub fn measure_temperature<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<f32, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_temperature_raw(delay, repeatability)
            .map(temperature_celsius_from_raw)
    }

    /// Triggers one measurement and reads only temperature in millidegrees Celsius.
    ///
    /// This is an integer-only alternative to [`Self::measure_temperature`].
    pub fn measure_temperature_millicelsius<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<i32, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_temperature_raw(delay, repeatability)
            .map(temperature_millicelsius_from_raw)
    }

    /// Triggers one measurement and reads only the raw temperature word.
    pub fn measure_temperature_raw<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<u16, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_temperature_raw_after_delay(delay, repeatability, repeatability.delay_ms())
    }

    /// Triggers one measurement without clock stretching at VDD below 2.4 V.
    pub fn measure_low_voltage<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<Measurement, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_raw_low_voltage(delay, repeatability)
            .map(RawMeasurement::to_measurement)
    }

    /// Triggers one raw measurement without clock stretching at VDD below 2.4 V.
    pub fn measure_raw_low_voltage<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_raw_after_delay(delay, repeatability, repeatability.low_voltage_delay_ms())
    }

    /// Triggers one low-voltage measurement and reads only the temperature word.
    pub fn measure_temperature_raw_low_voltage<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<u16, I2C::Error>
    where
        D: DelayNs,
    {
        self.measure_temperature_raw_after_delay(
            delay,
            repeatability,
            repeatability.low_voltage_delay_ms(),
        )
    }

    /// Triggers one measurement with clock stretching enabled.
    ///
    /// The I2C implementation must support clock stretching. ESP HAL I2C does.
    pub fn measure_with_clock_stretching(
        &mut self,
        repeatability: Repeatability,
    ) -> Result<Measurement, I2C::Error> {
        self.measure_raw_with_clock_stretching(repeatability)
            .map(RawMeasurement::to_measurement)
    }

    /// Triggers one raw measurement with clock stretching enabled.
    pub fn measure_raw_with_clock_stretching(
        &mut self,
        repeatability: Repeatability,
    ) -> Result<RawMeasurement, I2C::Error> {
        self.write_command(repeatability.single_shot_command(true))?;
        self.read_raw_measurement()
    }

    /// Triggers one clock-stretched measurement and reads only the temperature word.
    pub fn measure_temperature_raw_with_clock_stretching(
        &mut self,
        repeatability: Repeatability,
    ) -> Result<u16, I2C::Error> {
        self.write_command(repeatability.single_shot_command(true))?;
        self.read_raw_temperature()
    }

    /// Starts periodic acquisition.
    ///
    /// The SHT3x-DIS needs a 1 ms gap before it can receive the next command.
    /// Use [`Self::start_periodic_and_wait`] if the driver should enforce it.
    pub fn start_periodic(
        &mut self,
        repeatability: Repeatability,
        rate: PeriodicRate,
    ) -> Result<(), I2C::Error> {
        self.write_command(rate.command(repeatability))
    }

    /// Starts periodic acquisition and waits for the required command gap.
    pub fn start_periodic_and_wait<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
        rate: PeriodicRate,
    ) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command_and_wait(rate.command(repeatability), delay)
    }

    /// Starts periodic acquisition with accelerated response time.
    ///
    /// The SHT3x-DIS needs a 1 ms gap before it can receive the next command.
    /// Use [`Self::start_art_and_wait`] if the driver should enforce it.
    pub fn start_art(&mut self) -> Result<(), I2C::Error> {
        self.write_command(CMD_ART)
    }

    /// Starts ART mode and waits for the required command gap.
    pub fn start_art_and_wait<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command_and_wait(CMD_ART, delay)
    }

    /// Fetches one data pair from periodic acquisition.
    ///
    /// If no periodic sample is ready yet, the sensor responds to the read
    /// header with NACK. `embedded-hal` exposes that as `Error::I2c(_)`, so an
    /// I2C error from this method can mean "data not ready" rather than a
    /// wiring or bus fault.
    pub fn fetch(&mut self) -> Result<Measurement, I2C::Error> {
        self.fetch_raw().map(RawMeasurement::to_measurement)
    }

    /// Fetches one raw data pair from periodic acquisition.
    ///
    /// If no periodic sample is ready yet, the sensor responds to the read
    /// header with NACK. `embedded-hal` exposes that as `Error::I2c(_)`, so an
    /// I2C error from this method can mean "data not ready" rather than a
    /// wiring or bus fault.
    pub fn fetch_raw(&mut self) -> Result<RawMeasurement, I2C::Error> {
        self.write_command(CMD_FETCH_DATA)?;
        self.read_raw_measurement()
    }

    /// Stops periodic acquisition.
    ///
    /// The break command returns the sensor to single-shot mode in 1 ms.
    pub fn stop_periodic<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(CMD_BREAK)?;
        delay.delay_ms(1);
        Ok(())
    }

    /// Performs a device-specific soft reset.
    ///
    /// The SHT3x-DIS soft reset time has a 1.5 ms maximum, so this waits 2 ms.
    pub fn soft_reset<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(CMD_SOFT_RESET)?;
        delay.delay_ms(2);
        Ok(())
    }

    /// Performs an I2C general-call reset.
    ///
    /// This can reset every compatible device on the shared bus segment that
    /// responds to the general-call reset sequence. The reset is functionally
    /// identical to the dedicated reset pin, so this waits 2 ms to cover the
    /// worst-case power-up time.
    pub fn general_call_reset<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.i2c
            .write(GENERAL_CALL_ADDRESS, &[0x06])
            .map_err(Error::I2c)?;
        delay.delay_ms(2);
        Ok(())
    }

    /// Enables or disables the internal heater.
    ///
    /// The SHT3x-DIS needs a 1 ms gap before it can receive the next command.
    /// Use [`Self::set_heater_and_wait`] if the driver should enforce it.
    pub fn set_heater(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        let command = if enabled {
            CMD_HEATER_ENABLE
        } else {
            CMD_HEATER_DISABLE
        };
        self.write_command(command)
    }

    /// Enables or disables the internal heater and waits for the required command gap.
    pub fn set_heater_and_wait<D>(&mut self, delay: &mut D, enabled: bool) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        let command = if enabled {
            CMD_HEATER_ENABLE
        } else {
            CMD_HEATER_DISABLE
        };
        self.write_command_and_wait(command, delay)
    }

    /// Reads the status register.
    pub fn status(&mut self) -> Result<Status, I2C::Error> {
        let mut data = [0; 3];
        self.write_command(CMD_READ_STATUS)?;
        self.i2c.read(self.address, &mut data).map_err(Error::I2c)?;

        check_crc(DataWord::Status, data[0], data[1], data[2])?;
        Ok(Status(u16::from_be_bytes([data[0], data[1]])))
    }

    /// Clears status register flags.
    ///
    /// The SHT3x-DIS needs a 1 ms gap before it can receive the next command.
    /// Use [`Self::clear_status_and_wait`] if the driver should enforce it.
    pub fn clear_status(&mut self) -> Result<(), I2C::Error> {
        self.write_command(CMD_CLEAR_STATUS)
    }

    /// Clears status register flags and waits for the required command gap.
    pub fn clear_status_and_wait<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command_and_wait(CMD_CLEAR_STATUS, delay)
    }

    fn write_command(&mut self, command: u16) -> Result<(), I2C::Error> {
        self.i2c
            .write(self.address, &command.to_be_bytes())
            .map_err(Error::I2c)
    }

    fn write_command_and_wait<D>(&mut self, command: u16, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(command)?;
        delay.delay_ms(COMMAND_DELAY_MS);
        Ok(())
    }

    fn measure_raw_after_delay<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
        delay_ms: u32,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(repeatability.single_shot_command(false))?;
        delay.delay_ms(delay_ms);
        self.read_raw_measurement()
    }

    fn measure_temperature_raw_after_delay<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
        delay_ms: u32,
    ) -> Result<u16, I2C::Error>
    where
        D: DelayNs,
    {
        self.write_command(repeatability.single_shot_command(false))?;
        delay.delay_ms(delay_ms);
        self.read_raw_temperature()
    }

    fn read_raw_measurement(&mut self) -> Result<RawMeasurement, I2C::Error> {
        let mut data = [0; 6];
        self.i2c.read(self.address, &mut data).map_err(Error::I2c)?;
        parse_raw_measurement(data)
    }

    fn read_raw_temperature(&mut self) -> Result<u16, I2C::Error> {
        let mut data = [0; 3];
        self.i2c.read(self.address, &mut data).map_err(Error::I2c)?;
        parse_raw_temperature(data)
    }
}

#[cfg(feature = "async")]
impl<I2C> Sht3x<I2C>
where
    I2C: embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress>,
{
    /// Async version of [`Self::measure`].
    pub async fn measure_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<Measurement, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.measure_raw_async(delay, repeatability)
            .await
            .map(RawMeasurement::to_measurement)
    }

    /// Async version of [`Self::measure_raw`].
    pub async fn measure_raw_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.measure_raw_after_delay_async(delay, repeatability, repeatability.delay_ms())
            .await
    }

    /// Async version of [`Self::measure_temperature`].
    pub async fn measure_temperature_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<f32, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.measure_temperature_raw_async(delay, repeatability)
            .await
            .map(temperature_celsius_from_raw)
    }

    /// Async version of [`Self::measure_temperature_millicelsius`].
    pub async fn measure_temperature_millicelsius_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<i32, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.measure_temperature_raw_async(delay, repeatability)
            .await
            .map(temperature_millicelsius_from_raw)
    }

    /// Async version of [`Self::measure_temperature_raw`].
    pub async fn measure_temperature_raw_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<u16, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.measure_temperature_raw_after_delay_async(
            delay,
            repeatability,
            repeatability.delay_ms(),
        )
        .await
    }

    /// Async version of [`Self::measure_low_voltage`].
    pub async fn measure_low_voltage_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<Measurement, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.measure_raw_low_voltage_async(delay, repeatability)
            .await
            .map(RawMeasurement::to_measurement)
    }

    /// Async version of [`Self::measure_raw_low_voltage`].
    pub async fn measure_raw_low_voltage_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.measure_raw_after_delay_async(
            delay,
            repeatability,
            repeatability.low_voltage_delay_ms(),
        )
        .await
    }

    /// Async version of [`Self::measure_temperature_raw_low_voltage`].
    pub async fn measure_temperature_raw_low_voltage_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
    ) -> Result<u16, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.measure_temperature_raw_after_delay_async(
            delay,
            repeatability,
            repeatability.low_voltage_delay_ms(),
        )
        .await
    }

    /// Async version of [`Self::measure_with_clock_stretching`].
    pub async fn measure_with_clock_stretching_async(
        &mut self,
        repeatability: Repeatability,
    ) -> Result<Measurement, I2C::Error> {
        self.measure_raw_with_clock_stretching_async(repeatability)
            .await
            .map(RawMeasurement::to_measurement)
    }

    /// Async version of [`Self::measure_raw_with_clock_stretching`].
    pub async fn measure_raw_with_clock_stretching_async(
        &mut self,
        repeatability: Repeatability,
    ) -> Result<RawMeasurement, I2C::Error> {
        self.write_command_async(repeatability.single_shot_command(true))
            .await?;
        self.read_raw_measurement_async().await
    }

    /// Async version of [`Self::measure_temperature_raw_with_clock_stretching`].
    pub async fn measure_temperature_raw_with_clock_stretching_async(
        &mut self,
        repeatability: Repeatability,
    ) -> Result<u16, I2C::Error> {
        self.write_command_async(repeatability.single_shot_command(true))
            .await?;
        self.read_raw_temperature_async().await
    }

    /// Async version of [`Self::start_periodic`].
    pub async fn start_periodic_async(
        &mut self,
        repeatability: Repeatability,
        rate: PeriodicRate,
    ) -> Result<(), I2C::Error> {
        self.write_command_async(rate.command(repeatability)).await
    }

    /// Async version of [`Self::start_periodic_and_wait`].
    pub async fn start_periodic_and_wait_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
        rate: PeriodicRate,
    ) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_and_wait_async(rate.command(repeatability), delay)
            .await
    }

    /// Async version of [`Self::start_art`].
    pub async fn start_art_async(&mut self) -> Result<(), I2C::Error> {
        self.write_command_async(CMD_ART).await
    }

    /// Async version of [`Self::start_art_and_wait`].
    pub async fn start_art_and_wait_async<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_and_wait_async(CMD_ART, delay).await
    }

    /// Async version of [`Self::fetch`].
    ///
    /// If no periodic sample is ready yet, the sensor responds to the read
    /// header with NACK. `embedded-hal-async` exposes that as `Error::I2c(_)`.
    pub async fn fetch_async(&mut self) -> Result<Measurement, I2C::Error> {
        self.fetch_raw_async()
            .await
            .map(RawMeasurement::to_measurement)
    }

    /// Async version of [`Self::fetch_raw`].
    ///
    /// If no periodic sample is ready yet, the sensor responds to the read
    /// header with NACK. `embedded-hal-async` exposes that as `Error::I2c(_)`.
    pub async fn fetch_raw_async(&mut self) -> Result<RawMeasurement, I2C::Error> {
        self.write_command_async(CMD_FETCH_DATA).await?;
        self.read_raw_measurement_async().await
    }

    /// Async version of [`Self::stop_periodic`].
    pub async fn stop_periodic_async<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_async(CMD_BREAK).await?;
        delay.delay_ms(COMMAND_DELAY_MS).await;
        Ok(())
    }

    /// Async version of [`Self::soft_reset`].
    pub async fn soft_reset_async<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_async(CMD_SOFT_RESET).await?;
        delay.delay_ms(2).await;
        Ok(())
    }

    /// Async version of [`Self::general_call_reset`].
    pub async fn general_call_reset_async<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.i2c
            .write(GENERAL_CALL_ADDRESS, &[0x06])
            .await
            .map_err(Error::I2c)?;
        delay.delay_ms(2).await;
        Ok(())
    }

    /// Async version of [`Self::set_heater`].
    pub async fn set_heater_async(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        let command = if enabled {
            CMD_HEATER_ENABLE
        } else {
            CMD_HEATER_DISABLE
        };
        self.write_command_async(command).await
    }

    /// Async version of [`Self::set_heater_and_wait`].
    pub async fn set_heater_and_wait_async<D>(
        &mut self,
        delay: &mut D,
        enabled: bool,
    ) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        let command = if enabled {
            CMD_HEATER_ENABLE
        } else {
            CMD_HEATER_DISABLE
        };
        self.write_command_and_wait_async(command, delay).await
    }

    /// Async version of [`Self::status`].
    pub async fn status_async(&mut self) -> Result<Status, I2C::Error> {
        let mut data = [0; 3];
        self.write_command_async(CMD_READ_STATUS).await?;
        self.i2c
            .read(self.address, &mut data)
            .await
            .map_err(Error::I2c)?;

        check_crc(DataWord::Status, data[0], data[1], data[2])?;
        Ok(Status(u16::from_be_bytes([data[0], data[1]])))
    }

    /// Async version of [`Self::clear_status`].
    pub async fn clear_status_async(&mut self) -> Result<(), I2C::Error> {
        self.write_command_async(CMD_CLEAR_STATUS).await
    }

    /// Async version of [`Self::clear_status_and_wait`].
    pub async fn clear_status_and_wait_async<D>(&mut self, delay: &mut D) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_and_wait_async(CMD_CLEAR_STATUS, delay)
            .await
    }

    async fn write_command_async(&mut self, command: u16) -> Result<(), I2C::Error> {
        self.i2c
            .write(self.address, &command.to_be_bytes())
            .await
            .map_err(Error::I2c)
    }

    async fn write_command_and_wait_async<D>(
        &mut self,
        command: u16,
        delay: &mut D,
    ) -> Result<(), I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_async(command).await?;
        delay.delay_ms(COMMAND_DELAY_MS).await;
        Ok(())
    }

    async fn measure_raw_after_delay_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
        delay_ms: u32,
    ) -> Result<RawMeasurement, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_async(repeatability.single_shot_command(false))
            .await?;
        delay.delay_ms(delay_ms).await;
        self.read_raw_measurement_async().await
    }

    async fn measure_temperature_raw_after_delay_async<D>(
        &mut self,
        delay: &mut D,
        repeatability: Repeatability,
        delay_ms: u32,
    ) -> Result<u16, I2C::Error>
    where
        D: embedded_hal_async::delay::DelayNs,
    {
        self.write_command_async(repeatability.single_shot_command(false))
            .await?;
        delay.delay_ms(delay_ms).await;
        self.read_raw_temperature_async().await
    }

    async fn read_raw_measurement_async(&mut self) -> Result<RawMeasurement, I2C::Error> {
        let mut data = [0; 6];
        self.i2c
            .read(self.address, &mut data)
            .await
            .map_err(Error::I2c)?;
        parse_raw_measurement(data)
    }

    async fn read_raw_temperature_async(&mut self) -> Result<u16, I2C::Error> {
        let mut data = [0; 3];
        self.i2c
            .read(self.address, &mut data)
            .await
            .map_err(Error::I2c)?;
        parse_raw_temperature(data)
    }
}

fn parse_raw_measurement<E>(data: [u8; 6]) -> Result<RawMeasurement, E> {
    check_crc(DataWord::Temperature, data[0], data[1], data[2])?;
    check_crc(DataWord::Humidity, data[3], data[4], data[5])?;

    Ok(RawMeasurement {
        temperature: u16::from_be_bytes([data[0], data[1]]),
        humidity: u16::from_be_bytes([data[3], data[4]]),
    })
}

fn parse_raw_temperature<E>(data: [u8; 3]) -> Result<u16, E> {
    check_crc(DataWord::Temperature, data[0], data[1], data[2])?;
    Ok(u16::from_be_bytes([data[0], data[1]]))
}

fn temperature_celsius_from_raw(raw: u16) -> f32 {
    -45.0 + 175.0 * raw as f32 / MAX_RAW
}

fn temperature_fahrenheit_from_raw(raw: u16) -> f32 {
    -49.0 + 315.0 * raw as f32 / MAX_RAW
}

const fn temperature_millicelsius_from_raw(raw: u16) -> i32 {
    let raw = raw as i64;
    (-45_000 + (175_000 * raw) / 65_535) as i32
}

const fn temperature_millifahrenheit_from_raw(raw: u16) -> i32 {
    let raw = raw as i64;
    (-49_000 + (315_000 * raw) / 65_535) as i32
}

fn check_crc<E>(word: DataWord, msb: u8, lsb: u8, actual: u8) -> Result<(), E> {
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

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use core::convert::Infallible;
    #[cfg(feature = "async")]
    use core::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    use embedded_hal::{
        delay::DelayNs,
        i2c::{ErrorKind, ErrorType, Operation},
    };
    #[cfg(feature = "async")]
    use std::pin::pin;
    use std::vec::Vec;

    #[derive(Debug, Copy, Clone, Eq, PartialEq)]
    struct MockI2cError(ErrorKind);

    impl MockI2cError {
        const fn other() -> Self {
            Self(ErrorKind::Other)
        }
    }

    impl embedded_hal::i2c::Error for MockI2cError {
        fn kind(&self) -> ErrorKind {
            self.0
        }
    }

    #[derive(Debug)]
    enum ReadResponse {
        Data(Vec<u8>),
        Error(MockI2cError),
    }

    #[derive(Debug)]
    struct Write {
        address: u8,
        bytes: Vec<u8>,
    }

    #[derive(Debug)]
    struct MockI2c {
        writes: Vec<Write>,
        reads: Vec<ReadResponse>,
    }

    impl MockI2c {
        fn new(reads: impl IntoIterator<Item = Vec<u8>>) -> Self {
            Self {
                writes: Vec::new(),
                reads: reads.into_iter().map(ReadResponse::Data).collect(),
            }
        }

        fn with_read_responses(reads: impl IntoIterator<Item = ReadResponse>) -> Self {
            Self {
                writes: Vec::new(),
                reads: reads.into_iter().collect(),
            }
        }
    }

    impl ErrorType for MockI2c {
        type Error = MockI2cError;
    }

    impl I2c<SevenBitAddress> for MockI2c {
        fn transaction(
            &mut self,
            address: u8,
            operations: &mut [Operation<'_>],
        ) -> core::result::Result<(), Self::Error> {
            for operation in operations {
                match operation {
                    Operation::Read(buffer) => match self.reads.remove(0) {
                        ReadResponse::Data(read) => buffer.copy_from_slice(&read),
                        ReadResponse::Error(error) => return Err(error),
                    },
                    Operation::Write(bytes) => {
                        self.writes.push(Write {
                            address,
                            bytes: bytes.to_vec(),
                        });
                    }
                }
            }
            Ok(())
        }
    }

    #[cfg(feature = "async")]
    impl embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress> for MockI2c {
        async fn transaction(
            &mut self,
            address: u8,
            operations: &mut [embedded_hal_async::i2c::Operation<'_>],
        ) -> core::result::Result<(), Self::Error> {
            for operation in operations {
                match operation {
                    embedded_hal_async::i2c::Operation::Read(buffer) => {
                        match self.reads.remove(0) {
                            ReadResponse::Data(read) => buffer.copy_from_slice(&read),
                            ReadResponse::Error(error) => return Err(error),
                        }
                    }
                    embedded_hal_async::i2c::Operation::Write(bytes) => {
                        self.writes.push(Write {
                            address,
                            bytes: bytes.to_vec(),
                        });
                    }
                }
            }
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct MockDelay {
        delayed_ms: Vec<u32>,
    }

    impl DelayNs for MockDelay {
        fn delay_ns(&mut self, _ns: u32) {}

        fn delay_ms(&mut self, ms: u32) {
            self.delayed_ms.push(ms);
        }
    }

    #[cfg(feature = "async")]
    #[derive(Debug, Default)]
    struct MockAsyncDelay {
        delayed_ms: Vec<u32>,
    }

    #[cfg(feature = "async")]
    impl embedded_hal_async::delay::DelayNs for MockAsyncDelay {
        async fn delay_ns(&mut self, _ns: u32) {}

        async fn delay_ms(&mut self, ms: u32) {
            self.delayed_ms.push(ms);
        }
    }

    #[cfg(feature = "async")]
    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = pin!(future);

        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(output) => return output,
                Poll::Pending => core::hint::spin_loop(),
            }
        }
    }

    fn measurement_bytes(temperature: u16, humidity: u16) -> Vec<u8> {
        let [t_msb, t_lsb] = temperature.to_be_bytes();
        let [h_msb, h_lsb] = humidity.to_be_bytes();
        Vec::from([
            t_msb,
            t_lsb,
            crc8([t_msb, t_lsb]),
            h_msb,
            h_lsb,
            crc8([h_msb, h_lsb]),
        ])
    }

    fn temperature_bytes(temperature: u16) -> Vec<u8> {
        let [msb, lsb] = temperature.to_be_bytes();
        Vec::from([msb, lsb, crc8([msb, lsb])])
    }

    #[test]
    fn crc_matches_datasheet_example() {
        assert_eq!(crc8([0xBE, 0xEF]), 0x92);
    }

    #[test]
    fn parses_raw_measurement_and_converts_units() {
        let raw = parse_raw_measurement::<Infallible>(measurement_bytes(0, 0).try_into().unwrap())
            .unwrap();

        assert_eq!(raw.temperature, 0);
        assert_eq!(raw.humidity, 0);
        assert_eq!(raw.temperature_celsius(), -45.0);
        assert_eq!(raw.relative_humidity(), 0.0);

        let raw = parse_raw_measurement::<Infallible>(
            measurement_bytes(0xFFFF, 0xFFFF).try_into().unwrap(),
        )
        .unwrap();

        assert_eq!(raw.temperature_celsius(), 130.0);
        assert!((raw.temperature_fahrenheit() - 266.0).abs() < 0.001);
        assert_eq!(raw.relative_humidity(), 100.0);
    }

    #[test]
    fn converts_units_with_integer_only_math() {
        let raw = RawMeasurement {
            temperature: 0,
            humidity: 0,
        };

        assert_eq!(raw.temperature_millicelsius(), -45_000);
        assert_eq!(raw.temperature_millifahrenheit(), -49_000);
        assert_eq!(raw.relative_humidity_hundredths(), 0);
        assert_eq!(
            raw.to_fixed_point(),
            FixedPointMeasurement {
                temperature_millicelsius: -45_000,
                relative_humidity_hundredths: 0,
            }
        );

        let raw = RawMeasurement {
            temperature: 0xFFFF,
            humidity: 0xFFFF,
        };

        assert_eq!(raw.temperature_millicelsius(), 130_000);
        assert_eq!(raw.temperature_millifahrenheit(), 266_000);
        assert_eq!(raw.relative_humidity_hundredths(), 10_000);
    }

    #[test]
    fn rejects_bad_temperature_crc() {
        let mut bytes = measurement_bytes(0x1234, 0x5678);
        bytes[2] ^= 0x01;

        assert_eq!(
            parse_raw_measurement::<Infallible>(bytes.try_into().unwrap()),
            Err(Error::Crc {
                word: DataWord::Temperature,
                expected: crc8([0x12, 0x34]),
                actual: crc8([0x12, 0x34]) ^ 0x01,
            })
        );
    }

    #[test]
    fn rejects_bad_humidity_crc() {
        let mut bytes = measurement_bytes(0x1234, 0x5678);
        bytes[5] ^= 0x01;

        assert_eq!(
            parse_raw_measurement::<Infallible>(bytes.try_into().unwrap()),
            Err(Error::Crc {
                word: DataWord::Humidity,
                expected: crc8([0x56, 0x78]),
                actual: crc8([0x56, 0x78]) ^ 0x01,
            })
        );
    }

    #[test]
    fn rejects_bad_status_crc() {
        let status = 0x1234u16;
        let [msb, lsb] = status.to_be_bytes();
        let i2c = MockI2c::with_read_responses([ReadResponse::Data(Vec::from([
            msb,
            lsb,
            crc8([msb, lsb]) ^ 0x01,
        ]))]);
        let mut sensor = Sht3x::new(i2c);

        assert_eq!(
            sensor.status(),
            Err(Error::Crc {
                word: DataWord::Status,
                expected: crc8([msb, lsb]),
                actual: crc8([msb, lsb]) ^ 0x01,
            })
        );
    }

    #[test]
    fn formats_crc_errors_for_logs() {
        let error = Error::<Infallible>::Crc {
            word: DataWord::Humidity,
            expected: 0x92,
            actual: 0x93,
        };

        assert_eq!(
            std::format!("{error}"),
            "CRC mismatch for Humidity: expected 0x92, got 0x93"
        );
    }

    #[test]
    fn measures_single_shot_high_repeatability_without_clock_stretching() {
        let i2c = MockI2c::new([measurement_bytes(0x6666, 0x8000)]);
        let mut sensor = Sht3x::new(i2c);
        let mut delay = MockDelay::default();

        let raw = sensor.measure_raw(&mut delay, Repeatability::High).unwrap();
        let i2c = sensor.release();

        assert_eq!(raw.temperature, 0x6666);
        assert_eq!(raw.humidity, 0x8000);
        assert_eq!(delay.delayed_ms, Vec::from([15]));
        assert_eq!(i2c.writes.len(), 1);
        assert_eq!(i2c.writes[0].address, 0x44);
        assert_eq!(i2c.writes[0].bytes, Vec::from([0x24, 0x00]));
    }

    #[test]
    fn measures_temperature_only_with_short_read() {
        let i2c = MockI2c::new([temperature_bytes(0x6666)]);
        let mut sensor = Sht3x::new(i2c);
        let mut delay = MockDelay::default();

        let raw = sensor
            .measure_temperature_raw(&mut delay, Repeatability::High)
            .unwrap();
        let i2c = sensor.release();

        assert_eq!(raw, 0x6666);
        assert_eq!(delay.delayed_ms, Vec::from([15]));
        assert_eq!(i2c.writes[0].bytes, Vec::from([0x24, 0x00]));
    }

    #[test]
    fn low_voltage_measurement_uses_longer_delay() {
        let i2c = MockI2c::new([measurement_bytes(0x6666, 0x8000)]);
        let mut sensor = Sht3x::new(i2c);
        let mut delay = MockDelay::default();

        sensor
            .measure_raw_low_voltage(&mut delay, Repeatability::Medium)
            .unwrap();

        assert_eq!(delay.delayed_ms, Vec::from([7]));
    }

    #[test]
    fn repeatability_delay_tables_match_datasheet_maxima() {
        assert_eq!(Repeatability::Low.delay_ms(), 4);
        assert_eq!(Repeatability::Medium.delay_ms(), 6);
        assert_eq!(Repeatability::High.delay_ms(), 15);

        assert_eq!(Repeatability::Low.low_voltage_delay_ms(), 5);
        assert_eq!(Repeatability::Medium.low_voltage_delay_ms(), 7);
        assert_eq!(Repeatability::High.low_voltage_delay_ms(), 16);
    }

    #[test]
    fn periodic_commands_match_datasheet_table() {
        let expected = [
            (PeriodicRate::Mps0_5, Repeatability::High, 0x2032),
            (PeriodicRate::Mps0_5, Repeatability::Medium, 0x2024),
            (PeriodicRate::Mps0_5, Repeatability::Low, 0x202F),
            (PeriodicRate::Mps1, Repeatability::High, 0x2130),
            (PeriodicRate::Mps1, Repeatability::Medium, 0x2126),
            (PeriodicRate::Mps1, Repeatability::Low, 0x212D),
            (PeriodicRate::Mps2, Repeatability::High, 0x2236),
            (PeriodicRate::Mps2, Repeatability::Medium, 0x2220),
            (PeriodicRate::Mps2, Repeatability::Low, 0x222B),
            (PeriodicRate::Mps4, Repeatability::High, 0x2334),
            (PeriodicRate::Mps4, Repeatability::Medium, 0x2322),
            (PeriodicRate::Mps4, Repeatability::Low, 0x2329),
            (PeriodicRate::Mps10, Repeatability::High, 0x2737),
            (PeriodicRate::Mps10, Repeatability::Medium, 0x2721),
            (PeriodicRate::Mps10, Repeatability::Low, 0x272A),
        ];

        for (rate, repeatability, command) in expected {
            assert_eq!(rate.command(repeatability), command);
        }
    }

    #[test]
    fn uses_alternate_address_and_periodic_commands() {
        let i2c = MockI2c::new([measurement_bytes(0x1111, 0x2222)]);
        let mut sensor = Sht3x::with_address(i2c, Address::ALTERNATE);

        sensor
            .start_periodic(Repeatability::Medium, PeriodicRate::Mps4)
            .unwrap();
        let raw = sensor.fetch_raw().unwrap();
        let i2c = sensor.release();

        assert_eq!(raw.temperature, 0x1111);
        assert_eq!(raw.humidity, 0x2222);
        assert_eq!(i2c.writes[0].address, 0x45);
        assert_eq!(i2c.writes[0].bytes, Vec::from([0x23, 0x22]));
        assert_eq!(i2c.writes[1].bytes, Vec::from([0xE0, 0x00]));
    }

    #[test]
    fn fetch_propagates_not_ready_i2c_error() {
        let i2c = MockI2c::with_read_responses([ReadResponse::Error(MockI2cError::other())]);
        let mut sensor = Sht3x::new(i2c);

        assert_eq!(sensor.fetch_raw(), Err(Error::I2c(MockI2cError::other())));

        let i2c = sensor.release();
        assert_eq!(i2c.writes[0].bytes, Vec::from([0xE0, 0x00]));
    }

    #[test]
    fn configuration_wait_variants_enforce_command_gap() {
        let i2c = MockI2c::new([]);
        let mut sensor = Sht3x::new(i2c);
        let mut delay = MockDelay::default();

        sensor.clear_status_and_wait(&mut delay).unwrap();
        sensor.set_heater_and_wait(&mut delay, true).unwrap();
        sensor.start_art_and_wait(&mut delay).unwrap();
        sensor
            .start_periodic_and_wait(&mut delay, Repeatability::Low, PeriodicRate::Mps1)
            .unwrap();
        let i2c = sensor.release();

        assert_eq!(delay.delayed_ms, Vec::from([1, 1, 1, 1]));
        assert_eq!(i2c.writes[0].bytes, Vec::from([0x30, 0x41]));
        assert_eq!(i2c.writes[1].bytes, Vec::from([0x30, 0x6D]));
        assert_eq!(i2c.writes[2].bytes, Vec::from([0x2B, 0x32]));
        assert_eq!(i2c.writes[3].bytes, Vec::from([0x21, 0x2D]));
    }

    #[test]
    fn reads_status_and_exposes_bits() {
        let status = 0b1010_0000_0001_0011u16;
        let [msb, lsb] = status.to_be_bytes();
        let i2c = MockI2c::new([Vec::from([msb, lsb, crc8([msb, lsb])])]);
        let mut sensor = Sht3x::new(i2c);

        let status = sensor.status().unwrap();

        assert!(status.alert_pending());
        assert!(status.heater_enabled());
        assert!(status.reset_detected());
        assert!(status.command_failed());
        assert!(status.write_checksum_failed());
        assert!(!status.humidity_alert());
    }

    #[test]
    fn sends_general_call_reset_to_address_zero() {
        let i2c = MockI2c::new([]);
        let mut sensor = Sht3x::new(i2c);
        let mut delay = MockDelay::default();

        sensor.general_call_reset(&mut delay).unwrap();
        let i2c = sensor.release();

        assert_eq!(i2c.writes[0].address, 0x00);
        assert_eq!(i2c.writes[0].bytes, Vec::from([0x06]));
        assert_eq!(delay.delayed_ms, Vec::from([2]));
    }

    #[cfg(feature = "async")]
    #[test]
    fn async_measure_uses_single_shot_delay_and_reads_measurement() {
        let i2c = MockI2c::new([measurement_bytes(0x6666, 0x8000)]);
        let mut sensor = Sht3x::new(i2c);
        let mut delay = MockAsyncDelay::default();

        let raw = block_on(sensor.measure_raw_async(&mut delay, Repeatability::High)).unwrap();
        let i2c = sensor.release();

        assert_eq!(raw.temperature, 0x6666);
        assert_eq!(raw.humidity, 0x8000);
        assert_eq!(delay.delayed_ms, Vec::from([15]));
        assert_eq!(i2c.writes[0].bytes, Vec::from([0x24, 0x00]));
    }

    #[cfg(feature = "async")]
    #[test]
    fn async_periodic_commands_match_sync_sequence() {
        let i2c = MockI2c::new([measurement_bytes(0x1111, 0x2222)]);
        let mut sensor = Sht3x::with_address(i2c, Address::ALTERNATE);
        let mut delay = MockAsyncDelay::default();

        block_on(sensor.start_periodic_and_wait_async(
            &mut delay,
            Repeatability::Medium,
            PeriodicRate::Mps4,
        ))
        .unwrap();
        let raw = block_on(sensor.fetch_raw_async()).unwrap();
        let i2c = sensor.release();

        assert_eq!(raw.temperature, 0x1111);
        assert_eq!(raw.humidity, 0x2222);
        assert_eq!(delay.delayed_ms, Vec::from([1]));
        assert_eq!(i2c.writes[0].address, 0x45);
        assert_eq!(i2c.writes[0].bytes, Vec::from([0x23, 0x22]));
        assert_eq!(i2c.writes[1].bytes, Vec::from([0xE0, 0x00]));
    }

    #[cfg(feature = "async")]
    #[test]
    fn async_general_call_reset_waits_and_uses_address_zero() {
        let i2c = MockI2c::new([]);
        let mut sensor = Sht3x::new(i2c);
        let mut delay = MockAsyncDelay::default();

        block_on(sensor.general_call_reset_async(&mut delay)).unwrap();
        let i2c = sensor.release();

        assert_eq!(i2c.writes[0].address, 0x00);
        assert_eq!(i2c.writes[0].bytes, Vec::from([0x06]));
        assert_eq!(delay.delayed_ms, Vec::from([2]));
    }

    #[cfg(feature = "async")]
    #[test]
    fn async_status_reads_and_validates_crc() {
        let status = 0b1010_0000_0001_0011u16;
        let [msb, lsb] = status.to_be_bytes();
        let i2c = MockI2c::new([Vec::from([msb, lsb, crc8([msb, lsb])])]);
        let mut sensor = Sht3x::new(i2c);

        let status = block_on(sensor.status_async()).unwrap();

        assert!(status.alert_pending());
        assert!(status.heater_enabled());
        assert!(status.reset_detected());
        assert!(status.command_failed());
        assert!(status.write_checksum_failed());
        assert!(!status.humidity_alert());
    }

    #[cfg(feature = "async")]
    #[test]
    fn async_fetch_propagates_not_ready_i2c_error() {
        let i2c = MockI2c::with_read_responses([ReadResponse::Error(MockI2cError::other())]);
        let mut sensor = Sht3x::new(i2c);

        assert_eq!(
            block_on(sensor.fetch_raw_async()),
            Err(Error::I2c(MockI2cError::other()))
        );

        let i2c = sensor.release();
        assert_eq!(i2c.writes[0].bytes, Vec::from([0xE0, 0x00]));
    }

    #[test]
    fn reset_and_break_commands_wait_long_enough() {
        let i2c = MockI2c::new([]);
        let mut sensor = Sht3x::new(i2c);
        let mut delay = MockDelay::default();

        sensor.stop_periodic(&mut delay).unwrap();
        sensor.soft_reset(&mut delay).unwrap();

        let i2c = sensor.release();
        assert_eq!(i2c.writes[0].bytes, Vec::from([0x30, 0x93]));
        assert_eq!(i2c.writes[1].bytes, Vec::from([0x30, 0xA2]));
        assert_eq!(delay.delayed_ms, Vec::from([1, 2]));
    }

    #[test]
    fn custom_address_accepts_only_supported_sensor_addresses() {
        assert_eq!(Address::custom(0x44), Some(Address::DEFAULT));
        assert_eq!(Address::custom(0x45), Some(Address::ALTERNATE));
        assert_eq!(Address::custom(0x00), None);
        assert_eq!(Address::custom(0x43), None);
        assert_eq!(Address::custom(0x46), None);
        assert_eq!(Address::custom(0x7F), None);
    }

    #[test]
    fn address_default_is_sensor_default() {
        assert_eq!(Address::default(), Address::DEFAULT);
        assert_eq!(Address::default().as_u8(), 0x44);
    }

    #[test]
    fn driver_can_be_created_from_i2c_directly() {
        let sensor = Sht3x::from(MockI2c::new([]));
        assert_eq!(sensor.address(), 0x44);
    }

    fn assert_send<T: Send>() {}

    #[test]
    fn driver_is_send_when_i2c_is_send() {
        assert_send::<Sht3x<MockI2c>>();
    }
}
