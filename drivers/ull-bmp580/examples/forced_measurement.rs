use embedded_hal::delay::DelayNs;
use ull_bmp580::{Bmp580, Config, FixedPointMeasurement};

#[allow(dead_code)]
fn read_forced<I2C, D>(
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

fn main() {}
