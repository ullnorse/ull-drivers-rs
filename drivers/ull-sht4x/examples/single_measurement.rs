use embedded_hal::delay::DelayNs;
use ull_sht4x::{FixedPointMeasurement, Repeatability, Sht4x};

#[allow(dead_code)]
fn read_single_measurement<I2C, D>(
    i2c: I2C,
    delay: &mut D,
) -> Result<FixedPointMeasurement, ull_sht4x::Error<I2C::Error>>
where
    I2C: embedded_hal::i2c::I2c,
    D: DelayNs,
{
    let mut sensor = Sht4x::new(i2c);
    sensor.soft_reset(delay)?;
    Ok(sensor
        .measure_raw(delay, Repeatability::High)?
        .to_fixed_point())
}

fn main() {}
