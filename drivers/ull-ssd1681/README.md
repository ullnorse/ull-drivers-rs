# ull-ssd1681

`no_std` Rust driver for the SSD1681 e-paper controller on the Good Display
GDEY0154D67 panel (200x200, black/white), using `embedded-hal` 1.0.

## Transport

4-wire SPI, write only (the panel's SPI mode 0, at most 20 MHz write clock per
the SSD1681 datasheet p.37). The driver takes:

- an `embedded_hal::spi::SpiDevice` (chip select is the `SpiDevice`'s job)
- `OutputPin` for D/C and RST
- `InputPin` for BUSY (high while busy)
- `DelayNs`

## Usage

```rust,no_run
use ull_ssd1681::{DeepSleepMode, FRAME_LEN, Ssd1681};

fn show<SPI, DC, RST, BUSY, DELAY>(
    spi: SPI,
    dc: DC,
    rst: RST,
    busy: BUSY,
    delay: DELAY,
) -> Result<(), ull_ssd1681::Error<SPI::Error>>
where
    SPI: embedded_hal::spi::SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    let mut epd = Ssd1681::new(spi, dc, rst, busy, delay);
    epd.init()?;

    let mut frame = [0xFF; FRAME_LEN]; // all white
    frame[0] = 0x7F; // pixel (0, 0) black
    epd.display_full(&frame)?;

    frame[1] = 0x00; // pixels (8..16, 0) black
    epd.display_partial(&frame)?;

    epd.deep_sleep(DeepSleepMode::Mode1)?;
    Ok(())
}
```

## Frame format

`FRAME_LEN` = 5000 bytes: 200 rows top to bottom, 25 bytes per row, leftmost
pixel in bit 7. A set bit is white, a clear bit is black. This is the SSD1681
RAM layout with data entry mode `0x03`, and the layout of the ull-pocket UI
`Frame`.

## Refresh

- `full_refresh`: about 2 s, flickers. Panel waveform from the OTP, display
  mode 1.
- `partial_refresh`: about 0.26 s, no flicker, display mode 2. The new image
  must be in RAM `0x24` and the image on the panel in RAM `0x26`.
  `display_partial` does the whole sequence.
- Do a full refresh after 5 consecutive partial refreshes (panel datasheet
  p.22).
- Fast refresh (1.5 s, panel datasheet p.21) is not implemented: the panel
  datasheet gives no sequence for it. `write_command` is the escape hatch.

## Deep sleep

- Mode 1 keeps RAM but it is not accessible; mode 2 does not keep RAM.
- Leaving deep sleep needs a hardware reset. `wake` does reset plus
  configuration without touching RAM; `init` also fills both RAMs white.
- BUSY stays high in deep sleep.

## BUSY timeouts

Every wait polls BUSY once per millisecond and returns `Error::BusyTimeout`
when the limit in `Config` is exceeded. Defaults: 1 s for short commands, 6 s
for a full refresh (typical 2 s), 1.5 s for a partial refresh (typical
0.26 s).

## Async

The `async` feature adds `_async` methods using `embedded-hal-async` 1.0
(`SpiDevice` and `DelayNs`). BUSY is polled through the blocking `InputPin`.

## Datasheet notes

Command and timing doc comments cite page numbers of the SSD1681 datasheet
(Rev 0.13) and the GDEY0154D67 datasheet. The panel datasheet gives the init
flow (p.31) but not every parameter value; the values chosen for those are
documented next to the constants and need checking on hardware.
