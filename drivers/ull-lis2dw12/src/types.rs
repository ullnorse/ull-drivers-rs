// Datasheet: ST LIS2DW12, DocID029682 Rev 4 (LIS2DW12.pdf, 63 pages). Page
// numbers below are the printed page numbers, which match the PDF pages.

/// 7-bit I2C address with SA0 high, section 6.1.1 (p. 34). SA0 is also
/// internally pulled up (Table 3, p. 10), so a floating pin reads as this.
const DEFAULT_ADDRESS: u8 = 0x19;
/// 7-bit I2C address with SA0 tied to ground, section 6.1.1 (p. 34).
const ALTERNATE_ADDRESS: u8 = 0x18;

/// WHO_AM_I value, section 8.3 (p. 42).
pub(crate) const WHO_AM_I: u8 = 0x44;

/// CTRL1 (0x20) layout, section 8.4 (p. 43): ODR[3:0] bits 7:4, MODE[1:0]
/// bits 3:2, LP_MODE[1:0] bits 1:0.
const CTRL1_ODR_SHIFT: u8 = 4;
const CTRL1_MODE_SHIFT: u8 = 2;

/// CTRL6 (0x25) layout, section 8.9 (p. 48): BW_FILT[1:0] bits 7:6, FS[1:0]
/// bits 5:4, FDS bit 3, LOW_NOISE bit 2, bits 1:0 must be 0.
const CTRL6_BW_SHIFT: u8 = 6;
const CTRL6_FS_SHIFT: u8 = 4;
const CTRL6_LOW_NOISE: u8 = 1 << 2;

/// CTRL3 (0x22) interrupt pin bits, section 8.6 (p. 45).
const CTRL3_PP_OD: u8 = 1 << 5;
const CTRL3_LIR: u8 = 1 << 4;
const CTRL3_H_LACTIVE: u8 = 1 << 3;

/// WAKE_UP_THS (0x34) WK_THS[5:0], section 8.24 (p. 54).
const WAKE_THRESHOLD_MAX: u8 = 0x3F;
/// WAKE_UP_DUR (0x35) WAKE_DUR[1:0] bits 6:5, section 8.25 (p. 55).
const WAKE_DURATION_MAX: u8 = 0b11;
const WAKE_DURATION_SHIFT: u8 = 5;

/// Driver result type.
pub type Result<T, E> = core::result::Result<T, Error<E>>;

/// LIS2DW12 I2C address selection.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Address(u8);

impl Address {
    /// `0x19`, selected when SA0 is high (or floating, it has an internal pull-up).
    pub const DEFAULT: Self = Self(DEFAULT_ADDRESS);

    /// `0x18`, selected when SA0 is tied to GND.
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
    /// WHO_AM_I did not read `0x44`.
    UnexpectedChipId(u8),
    /// The requested configuration is not representable or not allowed.
    InvalidConfig,
    /// CTRL2.SOFT_RESET did not clear after a soft reset.
    ResetTimeout,
}

impl<BusError> core::fmt::Display for Error<BusError>
where
    BusError: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bus(error) => write!(f, "accelerometer bus error: {error:?}"),
            Self::UnexpectedChipId(id) => write!(f, "unexpected chip id 0x{id:02X}, expected 0x44"),
            Self::InvalidConfig => f.write_str("invalid configuration"),
            Self::ResetTimeout => f.write_str("soft reset did not complete"),
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

/// Output data rate, CTRL1 ODR[3:0] (Table 30, p. 43).
///
/// Several codes mean different rates in High-Performance and Low-Power mode;
/// see [`Self::millihertz`].
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum OutputDataRate {
    /// Power-down (code 0000).
    PowerDown = 0b0000,
    /// Code 0001: 12.5 Hz in High-Performance mode, 1.6 Hz in Low-Power mode.
    Hz1_6 = 0b0001,
    /// 12.5 Hz (code 0010).
    Hz12_5 = 0b0010,
    /// 25 Hz (code 0011).
    Hz25 = 0b0011,
    /// 50 Hz (code 0100).
    Hz50 = 0b0100,
    /// 100 Hz (code 0101).
    Hz100 = 0b0101,
    /// 200 Hz (code 0110).
    Hz200 = 0b0110,
    /// Code 0111: 400 Hz in High-Performance mode, 200 Hz in Low-Power mode.
    Hz400 = 0b0111,
    /// Code 1000: 800 Hz in High-Performance mode, 200 Hz in Low-Power mode.
    Hz800 = 0b1000,
    /// Code 1001: 1600 Hz in High-Performance mode, 200 Hz in Low-Power mode.
    Hz1600 = 0b1001,
}

impl OutputDataRate {
    /// Decodes ODR[3:0]. Codes above 1001 are not defined in Table 30.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Option<Self> {
        Some(match bits {
            0b0000 => Self::PowerDown,
            0b0001 => Self::Hz1_6,
            0b0010 => Self::Hz12_5,
            0b0011 => Self::Hz25,
            0b0100 => Self::Hz50,
            0b0101 => Self::Hz100,
            0b0110 => Self::Hz200,
            0b0111 => Self::Hz400,
            0b1000 => Self::Hz800,
            0b1001 => Self::Hz1600,
            _ => return None,
        })
    }

    /// Actual rate in millihertz for the given mode (Table 30, p. 43).
    /// Returns 0 for power-down.
    #[must_use]
    pub const fn millihertz(self, mode: OperatingMode) -> u32 {
        let high_performance = matches!(mode, OperatingMode::HighPerformance);
        match self {
            Self::PowerDown => 0,
            Self::Hz1_6 => {
                if high_performance {
                    12_500
                } else {
                    1_600
                }
            }
            Self::Hz12_5 => 12_500,
            Self::Hz25 => 25_000,
            Self::Hz50 => 50_000,
            Self::Hz100 => 100_000,
            Self::Hz200 => 200_000,
            Self::Hz400 => {
                if high_performance {
                    400_000
                } else {
                    200_000
                }
            }
            Self::Hz800 => {
                if high_performance {
                    800_000
                } else {
                    200_000
                }
            }
            Self::Hz1600 => {
                if high_performance {
                    1_600_000
                } else {
                    200_000
                }
            }
        }
    }
}

/// Operating mode, CTRL1 MODE[1:0] (Table 31, p. 43).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum OperatingMode {
    /// Low-Power mode (12/14-bit, set by [`LowPowerMode`]).
    LowPower = 0b00,
    /// High-Performance mode, 14-bit.
    HighPerformance = 0b01,
    /// Single data conversion on demand (12/14-bit, set by [`LowPowerMode`]).
    SingleConversion = 0b10,
}

/// Low-power mode, CTRL1 LP_MODE[1:0] (Table 32, p. 43).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum LowPowerMode {
    /// Low-Power mode 1, 12-bit resolution.
    Mode1 = 0b00,
    /// Low-Power mode 2, 14-bit resolution.
    Mode2 = 0b01,
    /// Low-Power mode 3, 14-bit resolution.
    Mode3 = 0b10,
    /// Low-Power mode 4, 14-bit resolution.
    Mode4 = 0b11,
}

impl LowPowerMode {
    const fn from_bits(bits: u8) -> Self {
        match bits & 0b11 {
            0b00 => Self::Mode1,
            0b01 => Self::Mode2,
            0b10 => Self::Mode3,
            _ => Self::Mode4,
        }
    }

    /// Typical turn-on time for single data conversion on demand in
    /// microseconds, section 3.2.2 (p. 22): 1.20, 1.70, 2.30, 3.55 ms.
    #[must_use]
    pub const fn single_conversion_time_us(self) -> u32 {
        match self {
            Self::Mode1 => 1_200,
            Self::Mode2 => 1_700,
            Self::Mode3 => 2_300,
            Self::Mode4 => 3_550,
        }
    }
}

/// Output resolution (Tables 11 and 31).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Resolution {
    /// 12-bit, Low-Power mode 1 only.
    Bits12,
    /// 14-bit.
    Bits14,
}

impl Resolution {
    /// The output words are left-justified 16-bit values (sections 8.12 to
    /// 8.17, pp. 49-51); this is the right shift that gives the digit count.
    const fn shift(self) -> u32 {
        match self {
            Self::Bits12 => 4,
            Self::Bits14 => 2,
        }
    }
}

/// Full-scale selection, CTRL6 FS[1:0] (Table 45, p. 48).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum FullScale {
    /// +-2 g.
    G2 = 0b00,
    /// +-4 g.
    G4 = 0b01,
    /// +-8 g.
    G8 = 0b10,
    /// +-16 g.
    G16 = 0b11,
}

impl FullScale {
    const fn from_bits(bits: u8) -> Self {
        match bits & 0b11 {
            0b00 => Self::G2,
            0b01 => Self::G4,
            0b10 => Self::G8,
            _ => Self::G16,
        }
    }

    /// Full-scale range in mg (the `+-` value: 2000, 4000, 8000, 16000).
    #[must_use]
    pub const fn milli_g(self) -> u32 {
        match self {
            Self::G2 => 2_000,
            Self::G4 => 4_000,
            Self::G8 => 8_000,
            Self::G16 => 16_000,
        }
    }

    /// Sensitivity in micro-g per digit, Table 4 (p. 11). The datasheet lists
    /// 0.244/0.488/0.976/1.952 mg/digit for 14-bit data and
    /// 0.976/1.952/3.904/7.808 mg/digit in Low-Power mode 1 (12-bit).
    #[must_use]
    pub const fn micro_g_per_digit(self, resolution: Resolution) -> i32 {
        match (resolution, self) {
            (Resolution::Bits14, Self::G2) => 244,
            (Resolution::Bits14, Self::G4) => 488,
            (Resolution::Bits14, Self::G8) => 976,
            (Resolution::Bits14, Self::G16) => 1_952,
            (Resolution::Bits12, Self::G2) => 976,
            (Resolution::Bits12, Self::G4) => 1_952,
            (Resolution::Bits12, Self::G8) => 3_904,
            (Resolution::Bits12, Self::G16) => 7_808,
        }
    }
}

/// Filter bandwidth, CTRL6 BW_FILT[1:0] (Table 44, p. 48).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum BandwidthFilter {
    /// ODR/2 (up to 800 Hz ODR, 400 Hz when ODR is 1600 Hz).
    OdrDiv2 = 0b00,
    /// ODR/4.
    OdrDiv4 = 0b01,
    /// ODR/10.
    OdrDiv10 = 0b10,
    /// ODR/20.
    OdrDiv20 = 0b11,
}

impl BandwidthFilter {
    const fn from_bits(bits: u8) -> Self {
        match bits & 0b11 {
            0b00 => Self::OdrDiv2,
            0b01 => Self::OdrDiv4,
            0b10 => Self::OdrDiv10,
            _ => Self::OdrDiv20,
        }
    }
}

/// Measurement configuration written to CTRL1 and CTRL6.
///
/// `Config::default()` equals the reset state of both registers (both `0x00`):
/// power-down, Low-Power mode 1, +-2 g, ODR/2 bandwidth, low-noise off.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Config {
    /// Output data rate. [`OutputDataRate::PowerDown`] stops measuring.
    pub odr: OutputDataRate,
    /// Operating mode.
    pub mode: OperatingMode,
    /// Low-power mode, used unless `mode` is High-Performance.
    pub low_power_mode: LowPowerMode,
    /// Full-scale range.
    pub full_scale: FullScale,
    /// Digital filter bandwidth.
    pub bandwidth: BandwidthFilter,
    /// CTRL6 LOW_NOISE.
    pub low_noise: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            odr: OutputDataRate::PowerDown,
            mode: OperatingMode::LowPower,
            low_power_mode: LowPowerMode::Mode1,
            full_scale: FullScale::G2,
            bandwidth: BandwidthFilter::OdrDiv2,
            low_noise: false,
        }
    }
}

impl Config {
    /// Resolution of the output data for this configuration.
    #[must_use]
    pub const fn resolution(&self) -> Resolution {
        match (self.mode, self.low_power_mode) {
            (OperatingMode::HighPerformance, _) => Resolution::Bits14,
            (_, LowPowerMode::Mode1) => Resolution::Bits12,
            _ => Resolution::Bits14,
        }
    }

    /// Actual output data rate in millihertz.
    #[must_use]
    pub const fn rate_millihertz(&self) -> u32 {
        self.odr.millihertz(self.mode)
    }

    /// CTRL1 register value.
    #[must_use]
    pub const fn ctrl1(&self) -> u8 {
        ((self.odr as u8) << CTRL1_ODR_SHIFT)
            | ((self.mode as u8) << CTRL1_MODE_SHIFT)
            | self.low_power_mode as u8
    }

    /// CTRL6 register value.
    #[must_use]
    pub const fn ctrl6(&self) -> u8 {
        ((self.bandwidth as u8) << CTRL6_BW_SHIFT)
            | ((self.full_scale as u8) << CTRL6_FS_SHIFT)
            | if self.low_noise { CTRL6_LOW_NOISE } else { 0 }
    }

    /// Decodes CTRL1 and CTRL6. Returns `None` for the reserved MODE `0b11`
    /// or an ODR code above `0b1001`.
    #[must_use]
    pub const fn from_registers(ctrl1: u8, ctrl6: u8) -> Option<Self> {
        let Some(odr) = OutputDataRate::from_bits(ctrl1 >> CTRL1_ODR_SHIFT) else {
            return None;
        };
        let mode = match (ctrl1 >> CTRL1_MODE_SHIFT) & 0b11 {
            0b00 => OperatingMode::LowPower,
            0b01 => OperatingMode::HighPerformance,
            0b10 => OperatingMode::SingleConversion,
            _ => return None,
        };
        Some(Self {
            odr,
            mode,
            low_power_mode: LowPowerMode::from_bits(ctrl1),
            full_scale: FullScale::from_bits(ctrl6 >> CTRL6_FS_SHIFT),
            bandwidth: BandwidthFilter::from_bits(ctrl6 >> CTRL6_BW_SHIFT),
            low_noise: ctrl6 & CTRL6_LOW_NOISE != 0,
        })
    }
}

/// Raw acceleration as read from the output registers: signed, left-justified
/// 16-bit words, 2's complement (sections 8.12 to 8.17, pp. 49-51).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct RawAcceleration {
    /// X axis.
    pub x: i16,
    /// Y axis.
    pub y: i16,
    /// Z axis.
    pub z: i16,
}

impl RawAcceleration {
    pub(crate) const fn from_bytes(data: [u8; 6]) -> Self {
        Self {
            x: i16::from_le_bytes([data[0], data[1]]),
            y: i16::from_le_bytes([data[2], data[3]]),
            z: i16::from_le_bytes([data[4], data[5]]),
        }
    }

    /// Converts to micro-g: the words are shifted right by 2 (14-bit) or 4
    /// (12-bit) to get the digit count, then multiplied by the Table 4
    /// sensitivity.
    #[must_use]
    pub const fn to_micro_g(self, full_scale: FullScale, resolution: Resolution) -> [i32; 3] {
        let shift = resolution.shift();
        let sensitivity = full_scale.micro_g_per_digit(resolution);
        [
            (self.x as i32 >> shift) * sensitivity,
            (self.y as i32 >> shift) * sensitivity,
            (self.z as i32 >> shift) * sensitivity,
        ]
    }

    /// Converts to mg as `f32`, see [`Self::to_micro_g`].
    #[must_use]
    pub fn to_milli_g(self, full_scale: FullScale, resolution: Resolution) -> [f32; 3] {
        let [x, y, z] = self.to_micro_g(full_scale, resolution);
        [x as f32 / 1000.0, y as f32 / 1000.0, z as f32 / 1000.0]
    }
}

/// STATUS register (0x27), section 8.11 (p. 49).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Status(pub u8);

impl Status {
    /// DRDY, bit 0: new X/Y/Z data available.
    #[must_use]
    pub const fn data_ready(self) -> bool {
        self.0 & (1 << 0) != 0
    }

    /// FF_IA, bit 1.
    #[must_use]
    pub const fn free_fall(self) -> bool {
        self.0 & (1 << 1) != 0
    }

    /// SLEEP_STATE, bit 5.
    #[must_use]
    pub const fn sleep_state(self) -> bool {
        self.0 & (1 << 5) != 0
    }

    /// WU_IA, bit 6: wake-up event detected.
    #[must_use]
    pub const fn wake_up(self) -> bool {
        self.0 & (1 << 6) != 0
    }
}

/// WAKE_UP_SRC register (0x38), section 8.28 (p. 57).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct WakeUpSource(pub u8);

impl WakeUpSource {
    /// Z_WU, bit 0: wake-up event on Z.
    #[must_use]
    pub const fn z(self) -> bool {
        self.0 & (1 << 0) != 0
    }

    /// Y_WU, bit 1: wake-up event on Y.
    #[must_use]
    pub const fn y(self) -> bool {
        self.0 & (1 << 1) != 0
    }

    /// X_WU, bit 2: wake-up event on X.
    #[must_use]
    pub const fn x(self) -> bool {
        self.0 & (1 << 2) != 0
    }

    /// WU_IA, bit 3: wake-up event detected.
    #[must_use]
    pub const fn wake_up(self) -> bool {
        self.0 & (1 << 3) != 0
    }

    /// SLEEP_STATE_IA, bit 4.
    #[must_use]
    pub const fn sleep_state(self) -> bool {
        self.0 & (1 << 4) != 0
    }

    /// FF_IA, bit 5: free-fall event detected.
    #[must_use]
    pub const fn free_fall(self) -> bool {
        self.0 & (1 << 5) != 0
    }
}

/// ALL_INT_SRC register (0x3B), section 8.31 (p. 58). Reading it resets all
/// interrupt flags routed to the INT pads.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct AllInterruptSource(pub u8);

impl AllInterruptSource {
    /// FF_IA, bit 0.
    #[must_use]
    pub const fn free_fall(self) -> bool {
        self.0 & (1 << 0) != 0
    }

    /// WU_IA, bit 1: wake-up event.
    #[must_use]
    pub const fn wake_up(self) -> bool {
        self.0 & (1 << 1) != 0
    }

    /// SLEEP_CHANGE_IA, bit 5.
    #[must_use]
    pub const fn sleep_change(self) -> bool {
        self.0 & (1 << 5) != 0
    }
}

/// Interrupt pin polarity, CTRL3 H_LACTIVE (p. 45). Reset is active high.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum InterruptPolarity {
    /// H_LACTIVE = 0 (reset value).
    ActiveHigh,
    /// H_LACTIVE = 1.
    ActiveLow,
}

/// Interrupt pad output type, CTRL3 PP_OD (p. 45). Reset is push-pull.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum InterruptOutput {
    /// PP_OD = 0 (reset value).
    PushPull,
    /// PP_OD = 1.
    OpenDrain,
}

/// Latched or pulsed interrupt requests, CTRL3 LIR (p. 45). Reset is pulsed.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum InterruptMode {
    /// LIR = 0 (reset value).
    Pulsed,
    /// LIR = 1: the request stays until the source register is read.
    Latched,
}

/// Interrupt pad configuration, the CTRL3 bits PP_OD, LIR and H_LACTIVE.
///
/// `InterruptPinConfig::default()` is the reset state: active high,
/// push-pull, pulsed.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct InterruptPinConfig {
    /// Pin polarity.
    pub polarity: InterruptPolarity,
    /// Push-pull or open-drain.
    pub output: InterruptOutput,
    /// Latched or pulsed.
    pub mode: InterruptMode,
}

impl Default for InterruptPinConfig {
    fn default() -> Self {
        Self {
            polarity: InterruptPolarity::ActiveHigh,
            output: InterruptOutput::PushPull,
            mode: InterruptMode::Pulsed,
        }
    }
}

impl InterruptPinConfig {
    /// The CTRL3 bits 5:3 for this configuration.
    #[must_use]
    pub const fn ctrl3_bits(&self) -> u8 {
        (match self.polarity {
            InterruptPolarity::ActiveHigh => 0,
            InterruptPolarity::ActiveLow => CTRL3_H_LACTIVE,
        }) | (match self.output {
            InterruptOutput::PushPull => 0,
            InterruptOutput::OpenDrain => CTRL3_PP_OD,
        }) | (match self.mode {
            InterruptMode::Pulsed => 0,
            InterruptMode::Latched => CTRL3_LIR,
        })
    }

    /// Decodes CTRL3 bits 5:3.
    #[must_use]
    pub const fn from_ctrl3(value: u8) -> Self {
        Self {
            polarity: if value & CTRL3_H_LACTIVE != 0 {
                InterruptPolarity::ActiveLow
            } else {
                InterruptPolarity::ActiveHigh
            },
            output: if value & CTRL3_PP_OD != 0 {
                InterruptOutput::OpenDrain
            } else {
                InterruptOutput::PushPull
            },
            mode: if value & CTRL3_LIR != 0 {
                InterruptMode::Latched
            } else {
                InterruptMode::Pulsed
            },
        }
    }
}

/// Wake-up detection settings for WAKE_UP_THS and WAKE_UP_DUR.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct WakeUpConfig {
    /// WK_THS[5:0], 0 to 63. 1 LSB is 1/64 of the full scale (p. 54).
    pub threshold: u8,
    /// WAKE_DUR[1:0], 0 to 3. 1 LSB is 1/ODR (p. 55).
    pub duration: u8,
}

impl WakeUpConfig {
    /// Builds a configuration from a threshold in mg, rounded to the nearest
    /// LSB of `full_scale / 64`. Returns `None` if the result is 0 or above
    /// 63, or `duration` is above 3.
    #[must_use]
    pub const fn from_milli_g(full_scale: FullScale, milli_g: u32, duration: u8) -> Option<Self> {
        let fs = full_scale.milli_g();
        let threshold = (milli_g.saturating_mul(64) + fs / 2) / fs;
        if threshold == 0 || threshold > WAKE_THRESHOLD_MAX as u32 || duration > WAKE_DURATION_MAX {
            None
        } else {
            Some(Self {
                threshold: threshold as u8,
                duration,
            })
        }
    }

    /// Threshold in micro-g for `full_scale` (`threshold * full_scale / 64`).
    #[must_use]
    pub const fn threshold_micro_g(&self, full_scale: FullScale) -> u32 {
        self.threshold as u32 * full_scale.milli_g() * 1000 / 64
    }

    /// WAKE_UP_THS and WAKE_UP_DUR register values, or `None` if a field is
    /// out of range. SLEEP_ON, SINGLE_DOUBLE_TAP, FF_DUR5, STATIONARY and
    /// SLEEP_DUR are written as 0.
    #[must_use]
    pub(crate) const fn registers(&self) -> Option<(u8, u8)> {
        if self.threshold > WAKE_THRESHOLD_MAX || self.duration > WAKE_DURATION_MAX {
            None
        } else {
            Some((self.threshold, self.duration << WAKE_DURATION_SHIFT))
        }
    }
}
