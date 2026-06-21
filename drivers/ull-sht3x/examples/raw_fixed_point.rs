use embedded_hal::delay::DelayNs;
use ull_sht3x::{FixedPointMeasurement, RawMeasurement, Repeatability, Sht3x};

#[allow(dead_code)]
fn read_fixed_point_measurement<I2C, D>(
    sensor: &mut Sht3x<I2C>,
    delay: &mut D,
) -> Result<FixedPointMeasurement, ull_sht3x::Error<I2C::Error>>
where
    I2C: embedded_hal::i2c::I2c,
    D: DelayNs,
{
    let raw = sensor.measure_raw(delay, Repeatability::High)?;
    Ok(raw.to_fixed_point())
}

#[allow(dead_code)]
fn convert_saved_raw_sample(raw: RawMeasurement) -> FixedPointMeasurement {
    raw.to_fixed_point()
}

fn main() {}
