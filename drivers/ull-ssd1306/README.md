# ull-ssd1306

`no_std` Rust SSD1306 driver using `embedded-hal` 1.0, with optional
`embedded-graphics-core` support.

The driver is built around three ideas:

- take an `embedded-hal` I2C device directly
- buffered drawing is explicit and predictable
- `embedded-graphics` integration is optional and never performs implicit bus I/O

## Features

- `no_std`
- typed I2C address selection
- typed panel sizes with size-specific initialization defaults
- size-aware `Page`, `DisplayLine`, `RowCount`, and `VerticalScrollArea` helpers
- optional `embedded-hal-async` support
- optional hardware-reset initialization helper
- raw command/data mode and buffered graphics mode
- full-frame and partial flush using horizontal addressing mode
- typed hardware scroll, vertical scroll area, start-line, and display-offset controls
- scroll-state typestate preventing RAM writes while hardware scrolling is active
- hardware orientation control without remapping framebuffer coordinates
- full segment-remap and COM-scan orientation control
- optional `embedded-graphics-core` integration
- optional `defmt` and `serde` derives on public data types

## Usage Sketch

```rust,no_run
use ull_ssd1306::{DisplaySize128x64, Rotation, Ssd1306};

fn init_display<I2C>(i2c: I2C) -> Result<(), ull_ssd1306::Error<I2C::Error>>
where
    I2C: embedded_hal::i2c::I2c,
{
    let mut display = Ssd1306::new(i2c, DisplaySize128x64, Rotation::Rotate0)
        .into_buffered_graphics_mode();

    display.init()?;
    display.clear();
    display.set_pixel(0, 0, true);
    display.flush()?;
    Ok(())
}
```

## Embedded Graphics

Enable the `graphics` feature to implement `DrawTarget<Color = BinaryColor>` on
buffered mode instances.

```toml
ull-ssd1306 = { path = "drivers/ull-ssd1306", features = ["graphics"] }
```

Drawing only updates the local framebuffer. Call `flush()` to send pixels to
the display. Use `flush_area()` to update only a page-aligned sub-region when
you want to reduce I2C traffic.

## Async

Enable the `async` feature to use `embedded-hal-async` 1.0 traits.

```toml
ull-ssd1306 = { path = "drivers/ull-ssd1306", features = ["async", "graphics"] }
```

Async methods mirror the blocking API with `_async` suffixes, for example
`init_async()`, `flush_async()`, and `write_command_async()`.

## Design Notes

- The public API takes `embedded-hal` I2C implementations directly and applies
  SSD1306 command/data framing internally.
- `init()` applies panel-specific multiplex/COM-pin defaults and uses the
  SSD1306 datasheet default contrast value `0x7F`.
- `init()` also programs the vertical scroll area to the active panel height,
  so diagonal scrolling starts from a valid `A3h` baseline on `128x64`, `128x32`,
  and `96x16` panels.
- `init_with_reset()` and `init_with_config_and_reset()` provide a datasheet-
  style `RES#` pulse for boards that expose the hardware reset pin.
- The framebuffer is owned by buffered mode. Raw mode has no pixel storage and
  exposes direct command and RAM-data operations.
- `Orientation` exposes the full hardware layout matrix, while `Rotation`
  remains available as the simple `0` and `180` degree convenience API.
- `start_scroll()` returns a scroll-active typestate and `stop_scroll()` returns
  the inactive one, which keeps `flush()` and raw RAM writes unavailable while
  hardware scrolling is active.
