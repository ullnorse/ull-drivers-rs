# ull-sht3x

`no_std` Rust driver for Sensirion SHT3x-DIS humidity and temperature sensors
using `embedded-hal` 1.0 I2C traits.

Supported parts include SHT30-DIS, SHT31-DIS, and SHT35-DIS. The driver is
designed to work with any platform whose I2C peripheral type implements
`embedded_hal::i2c::I2c`.

## Features

- Single-shot measurement with or without clock stretching
- Periodic acquisition, fetch, stop, and ART mode commands
- CRC-8 validation for temperature, humidity, and status words
- Raw `u16` readings, converted `f32` readings, and integer-only fixed-point readings
- Temperature-only reads that abort after the first 3-byte data word
- Status register helpers
- Soft reset, general-call reset, and heater control
- `no_std`
- Optional async API, `defmt`, and `serde` derives

## Usage Sketch

```rust,no_run
use embedded_hal::delay::DelayNs;
use ull_sht3x::{Repeatability, Sht3x};

fn read_sht3x<I2C, D>(i2c: I2C, delay: &mut D) -> Result<(), ull_sht3x::Error<I2C::Error>>
where
    I2C: embedded_hal::i2c::I2c,
    D: DelayNs,
{
    let mut sensor = Sht3x::new(i2c); // 0x44, ADDR tied low
    sensor.soft_reset(delay)?;
    sensor.clear_status_and_wait(delay)?;

    let reading = sensor.measure(delay, Repeatability::High)?;
    let temperature_c = reading.temperature_celsius;
    let humidity_rh = reading.relative_humidity;

    let _ = (temperature_c, humidity_rh);
    Ok(())
}
```

The SHT3x-DIS address is `0x44` when ADDR is tied low and `0x45` when ADDR is
tied high. As with other I2C devices, make sure your bus has the required
pull-up resistors and timing configuration for your target platform.

Use `Address::DEFAULT` or `Address::ALTERNATE` for the two standard address
strap options. For a dynamic 7-bit address, use `Address::custom(address)`.

## Fixed-Point Readings

`Measurement` uses `f32`, which is convenient on targets with hardware floating
point. For smaller `no_std` targets without an FPU, use the integer-only
conversion methods on `RawMeasurement`:

```rust,no_run
use embedded_hal::delay::DelayNs;
use ull_sht3x::{Repeatability, Sht3x};

fn read_fixed_point<I2C, D>(sensor: &mut Sht3x<I2C>, delay: &mut D) -> Result<(), ull_sht3x::Error<I2C::Error>>
where
    I2C: embedded_hal::i2c::I2c,
    D: DelayNs,
{
    let raw = sensor.measure_raw(delay, Repeatability::High)?;
    let fixed = raw.to_fixed_point();

    let temperature_millicelsius = fixed.temperature_millicelsius;
    let humidity_hundredths = fixed.relative_humidity_hundredths;

    let _ = (temperature_millicelsius, humidity_hundredths);
    Ok(())
}
```

For example, `21562` millidegrees Celsius means `21.562 deg C`, and `4512`
humidity hundredths means `45.12 %RH`.

## Async Support

Enable the `async` feature to use `embedded-hal-async` 1.0 traits. Async
measurement methods yield during conversion delays instead of blocking the
executor task.

```toml
ull-sht3x = { path = "drivers/ull-sht3x", features = ["async"] }
```

```rust,ignore
use ull_sht3x::{Repeatability, Sht3x};

async fn read_async<I2C, D>(i2c: I2C, delay: &mut D) -> Result<(), ull_sht3x::Error<I2C::Error>>
where
    I2C: embedded_hal_async::i2c::I2c,
    D: embedded_hal_async::delay::DelayNs,
{
    let mut sensor = Sht3x::new(i2c);
    sensor.soft_reset_async(delay).await?;

    let reading = sensor.measure_async(delay, Repeatability::High).await?;
    let temperature_only = sensor
        .measure_temperature_millicelsius_async(delay, Repeatability::High)
        .await?;

    let _ = (reading, temperature_only);
    Ok(())
}
```

Temperature-only methods such as `measure_temperature_raw` and
`measure_temperature_raw_async` request only the first three bytes from the
sensor, then stop before the humidity word. This follows the datasheet's early
read-abort allowance and reduces I2C bus time when humidity is not needed.

## Optional Features

Both features are disabled by default:

```toml
ull-sht3x = { path = "drivers/ull-sht3x", features = ["async", "defmt", "serde"] }
```

- `async`: enables async methods using `embedded-hal-async` 1.0.
- `defmt`: derives `defmt::Format` for public data and error types.
- `serde`: derives `Serialize` and `Deserialize` for public data and error types.

## Timing

For single-shot measurements without clock stretching, `measure_raw` and
`measure` wait for the datasheet maximum conversion time:

| Repeatability | Delay |
| --- | ---: |
| Low | 4 ms |
| Medium | 6 ms |
| High | 15 ms |

If VDD is below 2.4 V, use `Repeatability::low_voltage_delay_ms()` with your
own timing logic, or call `measure_raw_low_voltage` / `measure_low_voltage`.

After any command write, the datasheet requires a minimum 1 ms gap before the
sensor can receive another command. Methods such as `measure`, `soft_reset`,
`general_call_reset`, and `stop_periodic` already wait long enough. For
configuration commands that can be chained back-to-back, prefer
`clear_status_and_wait`, `set_heater_and_wait`, `start_art_and_wait`, and
`start_periodic_and_wait`. The shorter methods without `_and_wait` are kept for
callers that manage bus timing themselves.

When using periodic acquisition, `fetch` and `fetch_raw` can return
`Error::I2c(_)` if no sample is ready yet. The sensor signals that state by
NACKing the I2C read header, so this is not necessarily a wiring fault.

`PeriodicRate::Mps10` is supported, but the datasheet warns that self-heating
can occur at the highest measurement rate. That can shift temperature upward
and relative humidity downward. The size of the shift depends on airflow,
enclosure design, and PCB thermal layout.

`general_call_reset` writes the I2C general-call reset sequence to address
`0x00`. Any compatible device active on the same shared bus segment may reset,
not only the SHT3x instance represented by this driver.

The datasheet also describes an interface-only reset for a wedged bus: leave
SDA high and toggle SCL nine or more times before issuing a new START. This
requires direct GPIO control of the bus pins, so it cannot be implemented
through the generic `embedded_hal::i2c::I2c` trait. For noisy deployments,
consider designing your board support code so it can temporarily release I2C
and perform that pin-level recovery sequence.

## Datasheet Commands Covered

| Operation | Command |
| --- | ---: |
| Single shot, clock stretching enabled | `0x2C06`, `0x2C0D`, `0x2C10` |
| Single shot, clock stretching disabled | `0x2400`, `0x240B`, `0x2416` |
| Fetch data | `0xE000` |
| ART mode | `0x2B32` |
| Break / stop periodic | `0x3093` |
| Soft reset | `0x30A2` |
| Heater enable / disable | `0x306D`, `0x3066` |
| Read status / clear status | `0xF32D`, `0x3041` |
