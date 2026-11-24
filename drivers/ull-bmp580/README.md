# ull-bmp580

`no_std` Rust driver for the Bosch BMP580 pressure and temperature sensor using
`embedded-hal` 1.0 I2C traits.

The default address is `0x46` (SDO to GND); use `Address::ALTERNATE` for `0x47`
(SDO to VDDIO). SDO must not float. Written against datasheet BST-BMP580-DS004-02.

## Features

- Chip id check, soft reset, post-power-up checks (`init`)
- Power modes: standby, normal, forced, continuous
- Oversampling, output data rate and IIR filter configuration
- Forced measurement with the datasheet conversion time
- Raw 24-bit data, `f32`, and integer output (millidegrees Celsius, pascals and
  millipascals)
- Data-ready interrupt with explicit INT pin configuration
- Raw register read/write
- `no_std`, optional `async` and `defmt`

## Usage

```rust,no_run
use embedded_hal::delay::DelayNs;
use ull_bmp580::{Bmp580, Config, FixedPointMeasurement};

fn read<I2C, D>(
    i2c: I2C,
    delay: &mut D,
) -> Result<FixedPointMeasurement, ull_bmp580::Error<I2C::Error>>
where
    I2C: embedded_hal::i2c::I2c,
    D: DelayNs,
{
    let mut sensor = Bmp580::new(i2c);
    sensor.init(delay)?;
    sensor.configure(delay, &Config::default())?;
    Ok(sensor.measure_forced(delay)?.to_fixed_point())
}
```

## Optional Features

```toml
ull-bmp580 = { version = "0.1.0", features = ["async", "defmt"] }
```

- `async`: `*_async` methods using `embedded-hal-async` 1.0.
- `defmt`: derives `defmt::Format` for public data and error types.

## INT Pin

The reset value of INT_CONFIG is `0x35`: **latched**, **active low**,
**open drain**, INT pin **disabled**, drive strength 3. `InterruptConfig::default()`
is exactly that configuration, and `configure_data_ready_interrupt` only adds the
enable bit (writes `0x3D`). The driver never switches the pin to push-pull
unless you set `InterruptOutput::PushPull` yourself. It needs a pull-up.

In latched mode the pin stays asserted until `interrupt_status()` is read
(INT_STATUS clears on read), so read it after each wake. The pin is disabled
after power-up and does not signal the power-on-reset interrupt.

`configure_data_ready_interrupt` follows the datasheet's mode-switch sequence:
INT_SOURCE = 0, read INT_STATUS, write INT_CONFIG, then enable data ready.

## Timing And Hardware Notes

- Wait 2 ms after power-up before the first access. `init` soft-resets, checks
  CHIP_ID (`0x50`), checks STATUS (`nvm_rdy` set, `nvm_err` clear) and clears
  INT_STATUS.
- The soft reset write (`0xB6` to CMD) is not acknowledged by the sensor; the
  driver treats a NACK on that write as success, then waits 2 ms.
- Configuration registers are only writable in standby. `configure` goes to
  standby first (3 ms) and leaves the sensor in standby; modes can only be
  entered from standby.
- `measure_forced` waits 4 ms start-up (the deep-standby figure) plus the typical
  conversion times for the configured oversampling with a 5 % margin, rounded up
  (9 ms for the default 1x / 4x). If the sensor is not in standby it goes there
  first.
- Pressure and temperature are read in one burst from 0x1D so they belong to the
  same measurement. Pressure is only valid when `pressure_enabled` is set.
- Not every ODR/OSR combination is valid in normal mode. `odr_osr_valid` reads
  OSR_EFF.odr_is_valid; the sensor falls back to default oversampling when it is
  invalid.
