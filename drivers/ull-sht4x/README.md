# ull-sht4x

`no_std` Rust driver for Sensirion SHT4x (SHT40) humidity and temperature
sensors using `embedded-hal` 1.0 I2C traits.

Written against the SHT4x datasheet, version 2 (July 2021). The default address
is `0x44` (SHT40-AD1B); use `Address::ALTERNATE` for `0x45` (SHT40-BD1B).

## Features

- Single-shot measurement at high, medium or low repeatability
- CRC-8 check on every received word
- Raw ticks, `f32`, and integer fixed-point output (millidegrees Celsius,
  hundredths of %RH)
- Serial number and soft reset
- Heater commands as a raw escape hatch only
- `no_std`, optional `async` and `defmt`

## Usage

```rust,no_run
use embedded_hal::delay::DelayNs;
use ull_sht4x::{FixedPointMeasurement, Repeatability, Sht4x};

fn read<I2C, D>(
    i2c: I2C,
    delay: &mut D,
) -> Result<FixedPointMeasurement, ull_sht4x::Error<I2C::Error>>
where
    I2C: embedded_hal::i2c::I2c,
    D: DelayNs,
{
    let mut sensor = Sht4x::new(i2c);
    sensor.soft_reset(delay)?;
    let raw = sensor.measure_raw(delay, Repeatability::High)?;
    Ok(raw.to_fixed_point())
}
```

## Optional Features

```toml
ull-sht4x = { version = "0.1.0", features = ["async", "defmt"] }
```

- `async`: `*_async` methods using `embedded-hal-async` 1.0.
- `defmt`: derives `defmt::Format` for public data and error types.

## Timing And Hardware Notes

- The sensor does not clock-stretch. It NACKs the read header while a
  conversion is running, so each measurement waits the datasheet maximum,
  rounded up: 2 ms (low), 5 ms (medium), 9 ms (high). A bus NACK therefore
  means the delay was skipped or the wiring is wrong.
- Soft reset waits 1 ms. Power-up time after VDD reaches the POR level is up to
  1 ms; the driver does not wait for it.
- Fixed-point conversion truncates. Relative humidity is cropped to 0..=100 %RH
  as the datasheet recommends; the raw formula can return values outside that.
- The heater is rated for a duty cycle below 5 % and draws up to about 75 mA at
  200 mW. `measure_with_heater_raw` waits for the pulse plus a high-precision
  measurement (about 0.12 s or 1.11 s) and does not limit how often you call it.
