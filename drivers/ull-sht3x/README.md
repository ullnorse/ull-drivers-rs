# ull-sht3x

`no_std` Rust driver for Sensirion SHT3x-DIS humidity and temperature sensors
using `embedded-hal` 1.0 I2C traits.

Supported parts include SHT30-DIS, SHT31-DIS, and SHT35-DIS. The usual I2C
address is `0x44`; use `Address::ALTERNATE` for `0x45` when ADDR is tied high.

## Features

- Single-shot measurement with CRC-8 validation
- Raw `u16`, converted `f32`, and integer-only fixed-point readings
- Temperature-only reads when humidity is not needed
- Periodic acquisition and ART mode with typestate mode transitions
- Status register, soft reset, general-call reset, and heater control
- `no_std`
- Optional `async` and `defmt` support

## Usage

```rust,no_run
use embedded_hal::delay::DelayNs;
use ull_sht3x::{Measurement, Repeatability, Sht3x};

fn read_sht3x<I2C, D>(i2c: I2C, delay: &mut D) -> Result<Measurement, ull_sht3x::Error<I2C::Error>>
where
    I2C: embedded_hal::i2c::I2c,
    D: DelayNs,
{
    let mut sensor = Sht3x::new(i2c);
    sensor.soft_reset(delay)?;
    sensor.clear_status_and_wait(delay)?;
    sensor.measure(delay, Repeatability::High)
}
```

Use `measure_raw` when you want raw sensor words or integer-only conversion:

```rust,no_run
use embedded_hal::delay::DelayNs;
use ull_sht3x::{FixedPointMeasurement, Repeatability, Sht3x};

fn read_fixed_point<I2C, D>(
    sensor: &mut Sht3x<I2C>,
    delay: &mut D,
) -> Result<FixedPointMeasurement, ull_sht3x::Error<I2C::Error>>
where
    I2C: embedded_hal::i2c::I2c,
    D: DelayNs,
{
    let raw = sensor.measure_raw(delay, Repeatability::High)?;
    Ok(raw.to_fixed_point())
}
```

## Choosing A Method

- Use `measure` for most single-shot reads.
- Use `measure_raw` for raw data or fixed-point conversion.
- Use `measure_temperature`, `measure_temperature_millicelsius`, or
  `measure_temperature_raw` when humidity is not needed.
- Use `measure_with_clock_stretching` only if your I2C peripheral supports
  clock stretching.
- Use `_and_wait` configuration methods when you want the driver to enforce the
  datasheet's 1 ms command gap.

For periodic acquisition, call `start_periodic_and_wait`, then `fetch` or
`fetch_raw`, then `stop_periodic` before returning to single-shot commands. ART
mode follows the same shape with `start_art_and_wait`.

## Optional Features

All optional features are disabled by default:

```toml
ull-sht3x = { version = "0.1.0", features = ["async", "defmt"] }
```

- `async`: enables async methods using `embedded-hal-async` 1.0.
- `defmt`: derives `defmt::Format` for public data and error types.

## Timing And Hardware Notes

Single-shot measurements without clock stretching wait for the datasheet maximum
conversion time: 4 ms for low repeatability, 6 ms for medium, and 15 ms for
high. If VDD is below 2.4 V, use `Repeatability::low_voltage_delay_ms()` with
your own timing logic.

After most command writes, the sensor needs a 1 ms gap before receiving another
command. Helpers with `_and_wait` in the name enforce that gap.

`fetch` and `fetch_raw` return `Error::NotReady` when periodic data is not ready
yet. The sensor reports that by NACKing the I2C read header, so it is not
necessarily a wiring fault.

`general_call_reset` writes the I2C general-call reset sequence to address
`0x00`. Any compatible device on the same bus segment may reset, not only this
sensor.
