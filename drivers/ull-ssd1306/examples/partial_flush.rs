#![allow(dead_code)]

use embedded_hal::i2c::{I2c, SevenBitAddress};
use ull_ssd1306::{BufferedGraphicsMode, DisplaySize128x32, Result, Rotation, Ssd1306};

type BufferedDisplay<I2C> =
    Ssd1306<I2C, DisplaySize128x32, BufferedGraphicsMode<DisplaySize128x32>>;

fn init_display<I2C>(i2c: I2C) -> Result<BufferedDisplay<I2C>, I2C::Error>
where
    I2C: I2c<SevenBitAddress>,
{
    let mut display =
        Ssd1306::new(i2c, DisplaySize128x32, Rotation::Rotate0).into_buffered_graphics_mode();

    display.init()?;
    display.clear();
    display.flush()?;

    Ok(display)
}

fn update_status_bar<I2C>(
    display: &mut BufferedDisplay<I2C>,
    filled_columns: u32,
) -> Result<(), I2C::Error>
where
    I2C: I2c<SevenBitAddress>,
{
    let width = u32::from(display.width());
    let filled_columns = filled_columns.min(width);

    for x in 0..width {
        let on = x < filled_columns;

        for y in 0..8 {
            display.set_pixel(x, y, on);
        }
    }

    display.flush_area(0, 0, width, 8)
}

fn main() {}
