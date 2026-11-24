// Datasheet: Maxim MAX17048/MAX17049 (MAX17048.pdf, 19 pages). Page numbers
// below are the printed page numbers, which equal the PDF page numbers.

/// 7-bit slave address 0110110b = 0x36 (0x6C write / 0x6D read), "Slave
/// Address" section (p. 16). Fixed, no address pins.
const DEFAULT_ADDRESS: u8 = 0x36;

/// VCELL LSb = 78.125 uV per cell, Table 2 (p. 10).
const VCELL_NANOVOLTS_PER_LSB: u64 = 78_125;
/// SOC LSb = 1 %/256, Table 2 (p. 10).
const SOC_LSB_DIVISOR: u32 = 256;
/// CRATE LSb = 0.208 %/hr, Table 2 (p. 10) and CRATE section (p. 13).
const CRATE_MILLI_PERCENT_PER_LSB: i32 = 208;
/// VALRT LSb = 20 mV, VALRT section and Figure 11 (p. 13).
const VALRT_MV_PER_LSB: u16 = 20;

// STATUS register (0x1A) bits as seen in the 16-bit word (MSB first), Figure 13
// (p. 14): MSB = X, EnVR, SC, HD, VR, VL, VH, RI; LSB is all X (don't care).
pub(crate) const STATUS_ENVR: u16 = 1 << 14;
pub(crate) const STATUS_SC: u16 = 1 << 13;
pub(crate) const STATUS_HD: u16 = 1 << 12;
pub(crate) const STATUS_VR: u16 = 1 << 11;
pub(crate) const STATUS_VL: u16 = 1 << 10;
pub(crate) const STATUS_VH: u16 = 1 << 9;
pub(crate) const STATUS_RI: u16 = 1 << 8;
/// The alert indicator bits (everything that sets CONFIG.ALRT). RI and EnVR
/// are deliberately not part of this mask.
pub(crate) const STATUS_ALERT_FLAGS: u16 =
    STATUS_SC | STATUS_HD | STATUS_VR | STATUS_VL | STATUS_VH;

// CONFIG register (0x0C), Figure 10 (p. 12): MSB = RCOMP[7:0];
// LSB = SLEEP, ALSC, ALRT, ATHD[4:0].
pub(crate) const CONFIG_SLEEP: u16 = 1 << 7;
pub(crate) const CONFIG_ALSC: u16 = 1 << 6;
pub(crate) const CONFIG_ALRT: u16 = 1 << 5;
pub(crate) const CONFIG_ATHD_MASK: u16 = 0x1F;

/// Driver result type.
pub type Result<T, E> = core::result::Result<T, Error<E>>;

/// MAX17048 I2C address. The part has a single fixed address.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Address(u8);

impl Address {
    /// `0x36`, the only address the MAX17048 responds to.
    pub const DEFAULT: Self = Self(DEFAULT_ADDRESS);

    /// Creates an address from a supported 7-bit address (only `0x36`).
    #[must_use]
    pub const fn custom(address: u8) -> Option<Self> {
        if address == DEFAULT_ADDRESS {
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
    /// A threshold was outside the range the register can hold.
    InvalidThreshold,
}

impl<BusError> core::fmt::Display for Error<BusError>
where
    BusError: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bus(error) => write!(f, "fuel gauge bus error: {error:?}"),
            Self::InvalidThreshold => f.write_str("threshold out of range"),
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
            Self::InvalidThreshold => None,
        }
    }
}

/// VCELL register value to microvolts: `raw * 78.125 uV` (Table 2, p. 10).
///
/// The datasheet gives a 16-bit LSb and does not say how many low bits are
/// populated; this uses the register as a plain 16-bit value. Truncated.
#[must_use]
pub const fn vcell_microvolts(raw: u16) -> u32 {
    ((raw as u64 * VCELL_NANOVOLTS_PER_LSB) / 1000) as u32
}

/// SOC register value to hundredths of a percent: `raw / 256 %` (Table 2,
/// p. 10). `5000` means `50.00 %`. Truncated.
#[must_use]
pub const fn soc_hundredths(raw: u16) -> u16 {
    ((raw as u32 * 100) / SOC_LSB_DIVISOR) as u16
}

/// CRATE register value (two's complement) to thousandths of a percent per
/// hour: `raw * 0.208 %/hr` (Table 2, p. 10). Negative while discharging.
#[must_use]
pub const fn charge_rate_milli_percent_per_hour(raw: i16) -> i32 {
    raw as i32 * CRATE_MILLI_PERCENT_PER_LSB
}

/// STATUS register (0x1A), Figure 13 (p. 14).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Status(pub u16);

impl Status {
    /// RI: set at power-up. The IC is not configured while this is set.
    #[must_use]
    pub const fn reset_indicator(self) -> bool {
        self.0 & STATUS_RI != 0
    }

    /// VH: VCELL has been above `VALRT.MAX`.
    #[must_use]
    pub const fn voltage_high(self) -> bool {
        self.0 & STATUS_VH != 0
    }

    /// VL: VCELL has been below `VALRT.MIN`.
    #[must_use]
    pub const fn voltage_low(self) -> bool {
        self.0 & STATUS_VL != 0
    }

    /// VR: the device was reset (only if EnVr is set).
    #[must_use]
    pub const fn voltage_reset(self) -> bool {
        self.0 & STATUS_VR != 0
    }

    /// HD: SOC crossed `CONFIG.ATHD` (low-SOC alert).
    #[must_use]
    pub const fn soc_low(self) -> bool {
        self.0 & STATUS_HD != 0
    }

    /// SC: SOC changed by at least 1 % (only if `CONFIG.ALSC` is set).
    #[must_use]
    pub const fn soc_change(self) -> bool {
        self.0 & STATUS_SC != 0
    }

    /// EnVr: voltage-reset alert is enabled.
    #[must_use]
    pub const fn voltage_reset_alert_enabled(self) -> bool {
        self.0 & STATUS_ENVR != 0
    }

    /// `true` if any alert indicator (VH, VL, VR, HD, SC) is set.
    #[must_use]
    pub const fn any_alert(self) -> bool {
        self.0 & STATUS_ALERT_FLAGS != 0
    }
}

/// CONFIG register (0x0C), Figure 10 (p. 12).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Config(pub u16);

impl Config {
    /// Power-on value, Table 2 (p. 11).
    pub const RESET_VALUE: u16 = 0x971C;

    /// RCOMP compensation byte (POR value 0x97).
    #[must_use]
    pub const fn rcomp(self) -> u8 {
        (self.0 >> 8) as u8
    }

    /// Low-SOC alert threshold in percent: `32 - ATHD` (p. 12), 1..=32.
    #[must_use]
    pub const fn low_soc_threshold_percent(self) -> u8 {
        32 - (self.0 & CONFIG_ATHD_MASK) as u8
    }

    /// `true` while the ALRT bit is set (the ALRT pin is driven low).
    #[must_use]
    pub const fn alert(self) -> bool {
        self.0 & CONFIG_ALRT != 0
    }

    /// `true` if the 1 % SOC change alert is enabled.
    #[must_use]
    pub const fn soc_change_alert_enabled(self) -> bool {
        self.0 & CONFIG_ALSC != 0
    }

    /// `true` if the SLEEP bit is set.
    #[must_use]
    pub const fn sleep(self) -> bool {
        self.0 & CONFIG_SLEEP != 0
    }
}

/// Voltage alert window (VALRT, 0x14). The ALRT pin asserts while VCELL is
/// above `max_mv` or below `min_mv`. Resolution is 20 mV; values are
/// truncated to a multiple of 20 mV.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct VoltageAlert {
    /// Alert below this cell voltage in millivolts. `0` never alerts low.
    pub min_mv: u16,
    /// Alert above this cell voltage in millivolts. `5100` never alerts high.
    pub max_mv: u16,
}

impl VoltageAlert {
    /// Power-on value `0x00FF`: min 0 V, max 5.1 V, i.e. no voltage alerts.
    pub const DISABLED: Self = Self {
        min_mv: 0,
        max_mv: 255 * VALRT_MV_PER_LSB,
    };

    pub(crate) const fn to_register(self) -> Option<u16> {
        let min = self.min_mv / VALRT_MV_PER_LSB;
        let max = self.max_mv / VALRT_MV_PER_LSB;
        if min > 0xFF || max > 0xFF {
            None
        } else {
            Some((min << 8) | max)
        }
    }

    pub(crate) const fn from_register(raw: u16) -> Self {
        Self {
            min_mv: (raw >> 8) * VALRT_MV_PER_LSB,
            max_mv: (raw & 0xFF) * VALRT_MV_PER_LSB,
        }
    }
}
