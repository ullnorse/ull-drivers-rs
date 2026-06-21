#![allow(dead_code)]

use embedded_hal::i2c::{I2c, SevenBitAddress};
use ull_ssd1306::{BufferedGraphicsMode, DisplaySize128x64, Result, Rotation, Ssd1306};

type BufferedDisplay<I2C> =
    Ssd1306<I2C, DisplaySize128x64, BufferedGraphicsMode<DisplaySize128x64>>;

fn init_and_draw<I2C>(i2c: I2C) -> Result<BufferedDisplay<I2C>, I2C::Error>
where
    I2C: I2c<SevenBitAddress>,
{
    let mut display =
        Ssd1306::new(i2c, DisplaySize128x64, Rotation::Rotate0).into_buffered_graphics_mode();

    display.init()?;
    display.clear();
    draw_frame(&mut display);
    display.flush()?;

    Ok(display)
}

fn draw_frame<I2C>(display: &mut BufferedDisplay<I2C>)
where
    I2C: I2c<SevenBitAddress>,
{
    let width = u32::from(display.width());
    let height = u32::from(display.height());
    let diagonal = width.min(height);

    for x in 0..width {
        display.set_pixel(x, 0, true);
        display.set_pixel(x, height - 1, true);
    }

    for y in 0..height {
        display.set_pixel(0, y, true);
        display.set_pixel(width - 1, y, true);
    }

    for offset in 0..diagonal {
        display.set_pixel(offset, offset, true);
    }
}

fn main() {}
