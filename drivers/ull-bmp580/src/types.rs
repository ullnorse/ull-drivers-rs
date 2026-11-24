// Datasheet: Bosch BMP580, BST-BMP580-DS004-02 (BMP580.pdf, 74 pages). Page
// numbers below are PDF pages. Section numbers are the printed ones; the
// datasheet's own cross references use a different (3.x) numbering for the 4.x
// sections.

/// 7-bit I2C address with SDO = GND (7'h46), section 5.6 (p. 38).
const DEFAULT_ADDRESS: u8 = 0x46;
/// 7-bit I2C address with SDO = VDDIO (7'h47), section 5.6 (p. 38).
const ALTERNATE_ADDRESS: u8 = 0x47;

/// CHIP_ID reset value, register 0x01 (p. 51).
pub(crate) const CHIP_ID: u8 = 0x50;

/// ODR_CONFIG (0x37) layout, section 7.34 (p. 63-64): deep_dis bit 7, odr bits
/// 6:2, pwr_mode bits 1:0.
pub(crate) const ODR_DEEP_DIS: u8 = 1 << 7;
pub(crate) const ODR_SHIFT: u8 = 2;
pub(crate) const PWR_MODE_MASK: u8 = 0b11;

/// OSR_CONFIG (0x36) layout, section 7.33 (p. 63): press_en bit 6, osr_p bits
/// 5:3, osr_t bits 2:0.
const OSR_PRESS_EN: u8 = 1 << 6;
const OSR_P_SHIFT: u8 = 3;

/// INT_CONFIG (0x14) layout, section 7.5 (p. 52-53): int_mode bit 0, int_pol
/// bit 1, int_od bit 2, int_en bit 3, pad_int_drv bits 7:4. Reset value 0x35.
const INT_MODE_LATCHED: u8 = 1 << 0;
const INT_POL_ACTIVE_HIGH: u8 = 1 << 1;
const INT_OD_OPEN_DRAIN: u8 = 1 << 2;
const INT_EN: u8 = 1 << 3;
const INT_DRV_SHIFT: u8 = 4;
/// Reset value of INT_CONFIG: latched, active low, open drain, INT disabled,
/// pad_int_drv = 3 (p. 52).
#[cfg(test)]
pub(crate) const INT_CONFIG_RESET: u8 = 0x35;

/// INT_STATUS (0x27) bits, section 7.21 (p. 58).
const INT_STATUS_DRDY: u8 = 1 << 0;
const INT_STATUS_FIFO_FULL: u8 = 1 << 1;
const INT_STATUS_FIFO_THS: u8 = 1 << 2;
const INT_STATUS_OOR_P: u8 = 1 << 3;
const INT_STATUS_POR: u8 = 1 << 4;

/// STATUS (0x28) bits, section 7.22 (p. 58).
const STATUS_NVM_RDY: u8 = 1 << 1;
const STATUS_NVM_ERR: u8 = 1 << 2;

/// Driver result type.
pub type Result<T, E> = core::result::Result<T, Error<E>>;

/// BMP580 I2C address selection.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Address(u8);

impl Address {
    /// `0x46`, selected when SDO is tied to GND.
    pub const DEFAULT: Self = Self(DEFAULT_ADDRESS);

    /// `0x47`, selected when SDO is tied to VDDIO.
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

/// Driver errors.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error<BusError> {
    /// Bus error from the HAL.
    Bus(BusError),
    /// CHIP_ID did not read `0x50`.
    UnexpectedChipId(u8),
    /// STATUS reported the NVM not ready or in error after reset.
    Nvm,
    /// The requested configuration is not representable or not allowed.
    InvalidConfig,
}

impl<BusError> core::fmt::Display for Error<BusError>
where
    BusError: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bus(error) => write!(f, "sensor bus error: {error:?}"),
            Self::UnexpectedChipId(id) => write!(f, "unexpected chip id 0x{id:02X}, expected 0x50"),
            Self::Nvm => f.write_str("NVM not ready or reported an error"),
            Self::InvalidConfig => f.write_str("invalid configuration"),
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
            _ => None,
        }
    }
}

/// Oversampling ratio, OSR_CONFIG osr_t / osr_p (p. 63).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Oversampling {
    /// 1x (register value 0).
    X1 = 0,
    /// 2x.
    X2 = 1,
    /// 4x.
    X4 = 2,
    /// 8x.
    X8 = 3,
    /// 16x.
    X16 = 4,
    /// 32x.
    X32 = 5,
    /// 64x.
    X64 = 6,
    /// 128x (register value 7).
    X128 = 7,
}

impl Oversampling {
    pub(crate) const fn from_bits(bits: u8) -> Self {
        match bits & 0b111 {
            0 => Self::X1,
            1 => Self::X2,
            2 => Self::X4,
            3 => Self::X8,
            4 => Self::X16,
            5 => Self::X32,
            6 => Self::X64,
            _ => Self::X128,
        }
    }

    /// Typical pressure conversion time in microseconds, Table "Conversion
    /// time pressure" (p. 12): 1.0, 1.7, 2.9, 5.4, 10.4, 20.4, 40.4, 80.4 ms.
    const fn pressure_conversion_us(self) -> u32 {
        match self {
            Self::X1 => 1_000,
            Self::X2 => 1_700,
            Self::X4 => 2_900,
            Self::X8 => 5_400,
            Self::X16 => 10_400,
            Self::X32 => 20_400,
            Self::X64 => 40_400,
            Self::X128 => 80_400,
        }
    }

    /// Typical temperature conversion time in microseconds, Table "Conversion
    /// time temperature" (p. 12-13): 1.0, 1.1, 1.5, 2.1, 3.3, 5.8, 10.8, 20.8 ms.
    const fn temperature_conversion_us(self) -> u32 {
        match self {
            Self::X1 => 1_000,
            Self::X2 => 1_100,
            Self::X4 => 1_500,
            Self::X8 => 2_100,
            Self::X16 => 3_300,
            Self::X32 => 5_800,
            Self::X64 => 10_800,
            Self::X128 => 20_800,
        }
    }
}

/// Power mode, ODR_CONFIG pwr_mode (p. 63-64).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum PowerMode {
    /// No measurement ongoing. Configuration registers are writable.
    Standby = 0b00,
    /// Measurements on the configured ODR grid.
    Normal = 0b01,
    /// One measurement, then back to standby.
    Forced = 0b10,
    /// Back-to-back measurements without duty cycling ("Non-Stop" in the
    /// register map, "CONTINUOUS" in the text). The ODR setting is ignored.
    Continuous = 0b11,
}

impl PowerMode {
    pub(crate) const fn from_bits(bits: u8) -> Self {
        match bits & PWR_MODE_MASK {
            0b00 => Self::Standby,
            0b01 => Self::Normal,
            0b10 => Self::Forced,
            _ => Self::Continuous,
        }
    }
}

/// Output data rate, ODR_CONFIG odr (p. 64).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum OutputDataRate {
    /// 240.000 Hz (register value 0x00).
    Hz240 = 0x00,
    /// 218.537 Hz (register value 0x01).
    Hz218_537 = 0x01,
    /// 199.111 Hz (register value 0x02).
    Hz199_111 = 0x02,
    /// 179.200 Hz (register value 0x03).
    Hz179_2 = 0x03,
    /// 160.000 Hz (register value 0x04).
    Hz160 = 0x04,
    /// 149.333 Hz (register value 0x05).
    Hz149_333 = 0x05,
    /// 140.000 Hz (register value 0x06).
    Hz140 = 0x06,
    /// 129.855 Hz (register value 0x07).
    Hz129_855 = 0x07,
    /// 120.000 Hz (register value 0x08).
    Hz120 = 0x08,
    /// 110.164 Hz (register value 0x09).
    Hz110_164 = 0x09,
    /// 100.299 Hz (register value 0x0A).
    Hz100_299 = 0x0A,
    /// 89.600 Hz (register value 0x0B).
    Hz89_6 = 0x0B,
    /// 80.000 Hz (register value 0x0C).
    Hz80 = 0x0C,
    /// 70.000 Hz (register value 0x0D).
    Hz70 = 0x0D,
    /// 60.000 Hz (register value 0x0E).
    Hz60 = 0x0E,
    /// 50.056 Hz (register value 0x0F).
    Hz50_056 = 0x0F,
    /// 45.025 Hz (register value 0x10).
    Hz45_025 = 0x10,
    /// 40.000 Hz (register value 0x11).
    Hz40 = 0x11,
    /// 35.000 Hz (register value 0x12).
    Hz35 = 0x12,
    /// 30.000 Hz (register value 0x13).
    Hz30 = 0x13,
    /// 25.005 Hz (register value 0x14).
    Hz25_005 = 0x14,
    /// 20.000 Hz (register value 0x15).
    Hz20 = 0x15,
    /// 15.000 Hz (register value 0x16).
    Hz15 = 0x16,
    /// 10.000 Hz (register value 0x17).
    Hz10 = 0x17,
    /// 5.000 Hz (register value 0x18).
    Hz5 = 0x18,
    /// 4.000 Hz (register value 0x19).
    Hz4 = 0x19,
    /// 3.000 Hz (register value 0x1A).
    Hz3 = 0x1A,
    /// 2.000 Hz (register value 0x1B).
    Hz2 = 0x1B,
    /// 1.000 Hz (register value 0x1C).
    Hz1 = 0x1C,
    /// 0.500 Hz (register value 0x1D).
    Hz0_5 = 0x1D,
    /// 0.250 Hz (register value 0x1E).
    Hz0_25 = 0x1E,
    /// 0.125 Hz (register value 0x1F).
    Hz0_125 = 0x1F,
}

impl OutputDataRate {
    /// Register value (0x00..=0x1F).
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decodes a 5-bit ODR register value.
    #[must_use]
    pub const fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0x00 => Self::Hz240,
            0x01 => Self::Hz218_537,
            0x02 => Self::Hz199_111,
            0x03 => Self::Hz179_2,
            0x04 => Self::Hz160,
            0x05 => Self::Hz149_333,
            0x06 => Self::Hz140,
            0x07 => Self::Hz129_855,
            0x08 => Self::Hz120,
            0x09 => Self::Hz110_164,
            0x0A => Self::Hz100_299,
            0x0B => Self::Hz89_6,
            0x0C => Self::Hz80,
            0x0D => Self::Hz70,
            0x0E => Self::Hz60,
            0x0F => Self::Hz50_056,
            0x10 => Self::Hz45_025,
            0x11 => Self::Hz40,
            0x12 => Self::Hz35,
            0x13 => Self::Hz30,
            0x14 => Self::Hz25_005,
            0x15 => Self::Hz20,
            0x16 => Self::Hz15,
            0x17 => Self::Hz10,
            0x18 => Self::Hz5,
            0x19 => Self::Hz4,
            0x1A => Self::Hz3,
            0x1B => Self::Hz2,
            0x1C => Self::Hz1,
            0x1D => Self::Hz0_5,
            0x1E => Self::Hz0_25,
            0x1F => Self::Hz0_125,
            _ => return None,
        })
    }

    /// Output data rate in millihertz.
    #[must_use]
    pub const fn millihertz(self) -> u32 {
        match self {
            Self::Hz240 => 240000,
            Self::Hz218_537 => 218537,
            Self::Hz199_111 => 199111,
            Self::Hz179_2 => 179200,
            Self::Hz160 => 160000,
            Self::Hz149_333 => 149333,
            Self::Hz140 => 140000,
            Self::Hz129_855 => 129855,
            Self::Hz120 => 120000,
            Self::Hz110_164 => 110164,
            Self::Hz100_299 => 100299,
            Self::Hz89_6 => 89600,
            Self::Hz80 => 80000,
            Self::Hz70 => 70000,
            Self::Hz60 => 60000,
            Self::Hz50_056 => 50056,
            Self::Hz45_025 => 45025,
            Self::Hz40 => 40000,
            Self::Hz35 => 35000,
            Self::Hz30 => 30000,
            Self::Hz25_005 => 25005,
            Self::Hz20 => 20000,
            Self::Hz15 => 15000,
            Self::Hz10 => 10000,
            Self::Hz5 => 5000,
            Self::Hz4 => 4000,
            Self::Hz3 => 3000,
            Self::Hz2 => 2000,
            Self::Hz1 => 1000,
            Self::Hz0_5 => 500,
            Self::Hz0_25 => 250,
            Self::Hz0_125 => 125,
        }
    }
}

/// IIR filter coefficient for pressure and temperature, DSP_IIR (p. 61, 21).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum IirFilter {
    /// Filter bypassed (register value 0).
    Bypass = 0,
    /// Coefficient 1.
    Coeff1 = 1,
    /// Coefficient 3.
    Coeff3 = 2,
    /// Coefficient 7.
    Coeff7 = 3,
    /// Coefficient 15.
    Coeff15 = 4,
    /// Coefficient 31.
    Coeff31 = 5,
    /// Coefficient 63.
    Coeff63 = 6,
    /// Coefficient 127 (register value 7).
    Coeff127 = 7,
}

/// Measurement configuration written by `configure`.
///
/// `Config::default()` measures temperature and pressure with 1x / 4x
/// oversampling (the datasheet's "standard resolution" row), 1 Hz ODR, IIR
/// bypassed and deep standby allowed.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Config {
    /// Temperature oversampling.
    pub osr_temperature: Oversampling,
    /// Pressure oversampling.
    pub osr_pressure: Oversampling,
    /// Measure pressure as well as temperature (OSR_CONFIG.press_en). When
    /// `false` the sensor measures temperature only.
    pub pressure_enabled: bool,
    /// Output data rate for normal mode. Ignored in forced and continuous mode.
    pub odr: OutputDataRate,
    /// IIR filter for both channels. Deep standby needs this bypassed.
    pub iir: IirFilter,
    /// Sets ODR_CONFIG.deep_dis. Deep standby also needs ODR < 5 Hz and no FIFO
    /// (section 4.3.2, p. 17).
    pub deep_standby_disabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            osr_temperature: Oversampling::X1,
            osr_pressure: Oversampling::X4,
            pressure_enabled: true,
            odr: OutputDataRate::Hz1,
            iir: IirFilter::Bypass,
            deep_standby_disabled: false,
        }
    }
}

impl Config {
    /// OSR_CONFIG register value.
    #[must_use]
    pub const fn osr_register(&self) -> u8 {
        (if self.pressure_enabled {
            OSR_PRESS_EN
        } else {
            0
        }) | ((self.osr_pressure as u8) << OSR_P_SHIFT)
            | self.osr_temperature as u8
    }

    /// ODR_CONFIG register value for the given power mode.
    #[must_use]
    pub const fn odr_register(&self, mode: PowerMode) -> u8 {
        (if self.deep_standby_disabled {
            ODR_DEEP_DIS
        } else {
            0
        }) | (self.odr.as_u8() << ODR_SHIFT)
            | mode as u8
    }

    /// DSP_IIR register value (same coefficient for pressure and temperature).
    #[must_use]
    pub const fn iir_register(&self) -> u8 {
        ((self.iir as u8) << 3) | self.iir as u8
    }
}

/// INT pin trigger style, INT_CONFIG.int_mode (p. 52).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum InterruptMode {
    /// A pulse of about 105 us on each new event.
    Pulsed,
    /// Held asserted until INT_STATUS is read (data ready). This is the reset
    /// setting.
    Latched,
}

/// INT pin polarity, INT_CONFIG.int_pol (p. 52).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum InterruptPolarity {
    /// Asserted low. This is the reset setting.
    ActiveLow,
    /// Asserted high.
    ActiveHigh,
}

/// INT pin output stage, INT_CONFIG.int_od (p. 53).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum InterruptOutput {
    /// Open drain, needs a pull-up. This is the reset setting.
    OpenDrain,
    /// Push-pull.
    PushPull,
}

/// INT pin configuration (INT_CONFIG without the enable bit).
///
/// `InterruptConfig::default()` equals the reset configuration: latched, active
/// low, open drain, drive strength 3. Nothing here changes unless you set it,
/// so the pin stays open drain / active low unless you ask for push-pull.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct InterruptConfig {
    /// Pulsed or latched.
    pub mode: InterruptMode,
    /// Active low or active high.
    pub polarity: InterruptPolarity,
    /// Open drain or push-pull.
    pub output: InterruptOutput,
    /// Raw `pad_int_drv` field (0..=15). The datasheet says its MSB should only
    /// be set in open-drain configuration, so 8..=15 is rejected with push-pull.
    pub drive_strength: u8,
}

impl InterruptConfig {
    /// Reset configuration: latched, active low, open drain, drive 3.
    pub const RESET: Self = Self {
        mode: InterruptMode::Latched,
        polarity: InterruptPolarity::ActiveLow,
        output: InterruptOutput::OpenDrain,
        drive_strength: 3,
    };

    /// Encodes INT_CONFIG. Returns `None` for an invalid drive strength.
    #[must_use]
    pub const fn to_register(&self, enabled: bool) -> Option<u8> {
        if self.drive_strength > 0x0F
            || (self.drive_strength > 0x07 && matches!(self.output, InterruptOutput::PushPull))
        {
            return None;
        }
        let mut value = self.drive_strength << INT_DRV_SHIFT;
        if enabled {
            value |= INT_EN;
        }
        if matches!(self.output, InterruptOutput::OpenDrain) {
            value |= INT_OD_OPEN_DRAIN;
        }
        if matches!(self.polarity, InterruptPolarity::ActiveHigh) {
            value |= INT_POL_ACTIVE_HIGH;
        }
        if matches!(self.mode, InterruptMode::Latched) {
            value |= INT_MODE_LATCHED;
        }
        Some(value)
    }

    /// Decodes INT_CONFIG into the configuration and the enable bit.
    #[must_use]
    pub const fn from_register(value: u8) -> (Self, bool) {
        (
            Self {
                mode: if value & INT_MODE_LATCHED != 0 {
                    InterruptMode::Latched
                } else {
                    InterruptMode::Pulsed
                },
                polarity: if value & INT_POL_ACTIVE_HIGH != 0 {
                    InterruptPolarity::ActiveHigh
                } else {
                    InterruptPolarity::ActiveLow
                },
                output: if value & INT_OD_OPEN_DRAIN != 0 {
                    InterruptOutput::OpenDrain
                } else {
                    InterruptOutput::PushPull
                },
                drive_strength: value >> INT_DRV_SHIFT,
            },
            value & INT_EN != 0,
        )
    }
}

impl Default for InterruptConfig {
    fn default() -> Self {
        Self::RESET
    }
}

/// INT_STATUS (0x27), clear on read (p. 58).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct InterruptStatus(pub u8);

impl InterruptStatus {
    /// New pressure and/or temperature data is in the data registers.
    #[must_use]
    pub const fn data_ready(self) -> bool {
        self.0 & INT_STATUS_DRDY != 0
    }

    /// FIFO full.
    #[must_use]
    pub const fn fifo_full(self) -> bool {
        self.0 & INT_STATUS_FIFO_FULL != 0
    }

    /// FIFO threshold reached.
    #[must_use]
    pub const fn fifo_threshold(self) -> bool {
        self.0 & INT_STATUS_FIFO_THS != 0
    }

    /// Pressure out of range.
    #[must_use]
    pub const fn pressure_out_of_range(self) -> bool {
        self.0 & INT_STATUS_OOR_P != 0
    }

    /// Power-on reset or soft reset completed.
    #[must_use]
    pub const fn por(self) -> bool {
        self.0 & INT_STATUS_POR != 0
    }
}

pub(crate) const fn nvm_ok(status: u8) -> bool {
    status & STATUS_NVM_RDY != 0 && status & STATUS_NVM_ERR == 0
}

/// Raw measurement: sign-extended 24-bit data register values.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct RawMeasurement {
    /// Temperature, 1/65536 degC per LSb (signed 24 bit).
    pub temperature: i32,
    /// Pressure, 1/64 Pa per LSb (the register map says signed 24 bit). Only
    /// meaningful when pressure measurement is enabled.
    pub pressure: i32,
}

impl RawMeasurement {
    /// Builds a measurement from the six bytes at 0x1D..=0x22: temperature
    /// XLSB, LSB, MSB, pressure XLSB, LSB, MSB (pp. 56-57).
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 6]) -> Self {
        Self {
            temperature: sign_extend_24(bytes[0], bytes[1], bytes[2]),
            pressure: sign_extend_24(bytes[3], bytes[4], bytes[5]),
        }
    }

    /// Temperature in millidegrees Celsius: `raw / 2^16 degC` (section 4.5,
    /// p. 22). Fraction floored.
    #[must_use]
    pub const fn temperature_millicelsius(self) -> i32 {
        temperature_millicelsius(self.temperature)
    }

    /// Temperature in degrees Celsius.
    #[must_use]
    pub fn temperature_celsius(self) -> f32 {
        self.temperature as f32 / 65_536.0
    }

    /// Pressure in whole pascals: `raw / 2^6 Pa` (section 4.5, p. 22). Floored.
    #[must_use]
    pub const fn pressure_pa(self) -> i32 {
        self.pressure >> 6
    }

    /// Pressure in millipascals (`raw * 1000 / 64`, exact).
    #[must_use]
    pub const fn pressure_millipascal(self) -> i32 {
        ((self.pressure as i64 * 125) >> 3) as i32
    }

    /// Pressure in pascals as `f32`.
    #[must_use]
    pub fn pressure_pa_f32(self) -> f32 {
        self.pressure as f32 / 64.0
    }

    /// Converts to `f32` units.
    #[must_use]
    pub fn to_measurement(self) -> Measurement {
        Measurement {
            temperature_celsius: self.temperature_celsius(),
            pressure_pa: self.pressure_pa_f32(),
        }
    }

    /// Converts to integer units.
    #[must_use]
    pub const fn to_fixed_point(self) -> FixedPointMeasurement {
        FixedPointMeasurement {
            temperature_millicelsius: self.temperature_millicelsius(),
            pressure_millipascal: self.pressure_millipascal(),
        }
    }
}

/// Converted measurement in `f32`.
#[derive(Debug, Copy, Clone, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Measurement {
    /// Temperature in degrees Celsius.
    pub temperature_celsius: f32,
    /// Pressure in pascals.
    pub pressure_pa: f32,
}

/// Integer-only converted measurement.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct FixedPointMeasurement {
    /// Temperature in millidegrees Celsius. `25000` means `25.000 degC`.
    pub temperature_millicelsius: i32,
    /// Pressure in millipascals. `101_325_000` means `101325 Pa`.
    pub pressure_millipascal: i32,
}

pub(crate) const fn temperature_millicelsius(raw: i32) -> i32 {
    ((raw as i64 * 1000) >> 16) as i32
}

pub(crate) const fn sign_extend_24(xlsb: u8, lsb: u8, msb: u8) -> i32 {
    let value = ((msb as i32) << 16) | ((lsb as i32) << 8) | xlsb as i32;
    (value << 8) >> 8
}

/// Time to wait after triggering a forced measurement, in milliseconds.
///
/// Start-up from deep standby (4 ms, `tstartup_deep`, p. 11) plus the typical
/// conversion times for the configured oversampling with a 5 % margin (p. 12),
/// rounded up.
pub(crate) const fn forced_measurement_delay_ms(config_osr_register: u8) -> u32 {
    let osr_t = Oversampling::from_bits(config_osr_register);
    let osr_p = Oversampling::from_bits(config_osr_register >> OSR_P_SHIFT);
    let mut conversion_us = osr_t.temperature_conversion_us();
    if config_osr_register & OSR_PRESS_EN != 0 {
        conversion_us += osr_p.pressure_conversion_us();
    }
    let total_us = 4_000 + conversion_us * 105 / 100;
    total_us.div_ceil(1000)
}
