#![allow(dead_code)]

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;
use ull_ssd1681::{DeepSleepMode, FRAME_LEN, PARTIALS_BEFORE_FULL, Result, Ssd1681};

/// Shows a frame, updates it a few times, then sleeps with RAM kept.
fn show_and_sleep<SPI, DC, RST, BUSY, DELAY>(
    spi: SPI,
    dc: DC,
    rst: RST,
    busy: BUSY,
    delay: DELAY,
) -> Result<(), SPI::Error>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    DELAY: DelayNs,
{
    let mut epd = Ssd1681::new(spi, dc, rst, busy, delay);
    epd.init()?;

    let mut frame = [0xFF; FRAME_LEN];
    epd.display_full(&frame)?;

    for step in 0..PARTIALS_BEFORE_FULL {
        frame[usize::from(step)] = 0x00;
        epd.display_partial(&frame)?;
    }

    // Mode 1 keeps RAM; `wake` brings the panel back without rewriting it.
    epd.deep_sleep(DeepSleepMode::Mode1)
}

fn main() {}
