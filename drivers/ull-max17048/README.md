# ull-max17048

`no_std` Rust driver for the Maxim MAX17048 single-cell ModelGauge fuel gauge
using `embedded-hal` 1.0 I2C traits.

The address is fixed at `0x36`. Written against the MAX17048/MAX17049 datasheet.

## Features

- Cell voltage (VCELL), state of charge (SOC), charge rate (CRATE), version
- Fixed-point (microvolts, hundredths of a percent, milli-percent per hour) and
  `f32` conversions
- Alerts: low-SOC threshold (`CONFIG.ATHD`), voltage window (`VALRT`), SOC-change
  alert, status flags, and a single `clear_alert` call
- Hibernate control (`HIBRT`) and sleep mode
- Raw 16-bit register read/write
- `no_std`, optional `async` and `defmt`

## Usage

```rust,no_run
use ull_max17048::{Max17048, VoltageAlert};

fn setup<I2C: embedded_hal::i2c::I2c>(
    i2c: I2C,
) -> Result<u16, ull_max17048::Error<I2C::Error>> {
    let mut gauge = Max17048::new(i2c);
    gauge.set_low_soc_threshold(10)?;
    gauge.set_voltage_alert(VoltageAlert { min_mv: 3300, max_mv: 4300 })?;
    gauge.soc_hundredths()
}
```

Before deep sleep, deassert the ALRT pin:

```rust,no_run
fn before_sleep<I2C: embedded_hal::i2c::I2c>(
    gauge: &mut ull_max17048::Max17048<I2C>,
) -> Result<(), ull_max17048::Error<I2C::Error>> {
    let pending = gauge.clear_alert()?;
    if pending.soc_low() {
        // remember that the battery was low
    }
    Ok(())
}
```

## Optional Features

```toml
ull-max17048 = { version = "0.1.0", features = ["async", "defmt"] }
```

- `async`: `*_async` methods using `embedded-hal-async` 1.0.
- `defmt`: derives `defmt::Format` for public data and error types.

## Hardware Notes

- ALRT is active low and open drain; the board needs a pull-up. It stays low
  until `CONFIG.ALRT` is written to 0. Clearing only the STATUS flags does not
  release it. `clear_alert` clears the STATUS alert flags (VH, VL, VR, HD, SC)
  and then `CONFIG.ALRT`, preserving every other bit of both registers, and
  returns the STATUS that was pending.
- The alerts are enabled at power-up, so ALRT can already be low at boot.
- The datasheet does not say whether ALRT re-asserts while a voltage condition
  still holds after clearing. Assume it can on the next sample (every 250 ms in
  active mode, every 45 s in hibernate). If ALRT is a wake source, read
  `alert_asserted` after clearing, and clear again after the voltage condition
  has gone.
- The low-SOC alert (`ATHD`) triggers only on a falling edge past the
  threshold.
- Registers are 16-bit and transferred as whole words; the driver always does
  one write-then-read transaction or one three-byte write. Writes of one byte
  are ignored by the IC.
- Sleep mode halts the gauge (no self-discharge tracking) and does not clear
  alerts. Only `CONFIG.SLEEP = 0` wakes it. `MODE.EnSleep` also lets the IC
  sleep when SDA and SCL stay low for 2.5 s, so keep EnSleep clear if the MCU
  may hold the bus low while it deep-sleeps.
- `VCELL` is converted as a plain 16-bit value at 78.125 uV per LSb, `CRATE` as a
  signed 16-bit value at 0.208 %/hr per LSb (the datasheet does not spell out
  the sign encoding).
- The first SOC value is available about 1 s after power-up. A freshly powered
  gauge has `STATUS.RI` set; use `clear_reset_indicator` once configured.
