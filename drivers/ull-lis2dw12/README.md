# ull-lis2dw12

`no_std` Rust driver for the ST LIS2DW12 3-axis accelerometer using
`embedded-hal` 1.0 I2C traits.

The default address is `0x19` (SA0 high, which is also what the internal pull-up
gives); use `Address::ALTERNATE` for `0x18` (SA0 to GND). CS must be tied high to
select I2C. Written against datasheet DocID029682 Rev 4.

## Features

- WHO_AM_I check (`0x44`) and soft reset
- ODR, operating mode, low-power mode, full-scale, bandwidth and low-noise config
- Raw acceleration, micro-g (integer) and mg (`f32`) output
- 8-bit temperature output
- Wake-up interrupt on INT1 with threshold and duration
- Interrupt polarity, push-pull/open-drain and latched/pulsed selection
- Read and clear of the wake-up source
- Raw register read/write
- `no_std`, optional `async` and `defmt`

## Usage

```rust,no_run
use embedded_hal::delay::DelayNs;
use ull_lis2dw12::{Config, Lis2dw12, OperatingMode, OutputDataRate};

fn read<I2C, D>(i2c: I2C, delay: &mut D) -> Result<[f32; 3], ull_lis2dw12::Error<I2C::Error>>
where
    I2C: embedded_hal::i2c::I2c,
    D: DelayNs,
{
    let mut sensor = Lis2dw12::new(i2c);
    let config = Config {
        odr: OutputDataRate::Hz50,
        mode: OperatingMode::HighPerformance,
        ..Config::default()
    };
    sensor.init(delay)?;
    sensor.configure(&config)?;
    sensor.read_acceleration_mg(&config)
}
```

## Optional Features

```toml
ull-lis2dw12 = { version = "0.1.0", features = ["async", "defmt"] }
```

- `async`: `*_async` methods using `embedded-hal-async` 1.0.
- `defmt`: derives `defmt::Format` for public data and error types.

## Wake On Motion

```rust,no_run
# use embedded_hal::delay::DelayNs;
# use ull_lis2dw12::*;
# fn arm<I2C: embedded_hal::i2c::I2c, D: DelayNs>(i2c: I2C, delay: &mut D) -> ull_lis2dw12::Result<(), I2C::Error> {
# let mut sensor = Lis2dw12::new(i2c);
sensor.set_interrupt_pin_config(&InterruptPinConfig {
    polarity: InterruptPolarity::ActiveLow,
    mode: InterruptMode::Latched,
    ..InterruptPinConfig::default()
})?;
let wake = WakeUpConfig::from_milli_g(FullScale::G2, 500, 2).ok_or(Error::InvalidConfig)?;
sensor.enable_wake_up_interrupt(&wake)?;
# Ok(()) }
```

- The reset state of CTRL3 is **active high**, **push-pull**, **pulsed**. A board
  that needs an active-low INT1 must call `set_interrupt_pin_config`.
- The wake-up threshold LSB is full-scale / 64 (31.25 mg at +-2 g). The duration
  LSB is 1/ODR, so the time depends on the configured data rate. The sensor has
  to be running (ODR above power-down) for wake-up to be detected.
- `enable_wake_up_interrupt` writes WAKE_UP_THS and WAKE_UP_DUR, routes
  INT1_WU, sets CTRL7.INTERRUPTS_ENABLE and finally reads ALL_INT_SRC so a stale
  flag does not hold INT1 asserted.
- In latched mode INT1 stays asserted until ALL_INT_SRC is read:
  `clear_interrupts()` returns it. `read_wake_up_source()` reads WAKE_UP_SRC
  (which axis); the datasheet only states that ALL_INT_SRC releases the flags.

## Timing And Hardware Notes

- The datasheet gives no power-up or soft-reset time. `soft_reset` sets
  CTRL2.SOFT_RESET (with IF_ADD_INC kept set), then polls CTRL2 every millisecond
  until the bit clears, up to 10 times (`Error::ResetTimeout`).
- Output words are left-justified 16-bit. 14-bit data (all modes except
  Low-Power mode 1) is shifted right by 2, 12-bit data by 4, before multiplying
  by the Table 4 sensitivity (0.244 mg/digit at +-2 g, 14-bit).
- `read_acceleration` is a six-byte burst, which relies on CTRL2.IF_ADD_INC
  (set after reset).
- `trigger_single_conversion` starts one on-demand conversion; wait
  `LowPowerMode::single_conversion_time_us` before reading.
