use embedded_hal::delay::DelayNs;
use ull_sht3x::{Measurement, Repeatability, Sht3x};

#[allow(dead_code)]
fn read_single_measurement<I2C, D>(
    i2c: I2C,
    delay: &mut D,
) -> Result<Measurement, ull_sht3x::Error<I2C::Error>>
where
    I2C: embedded_hal::i2c::I2c,
    D: DelayNs,
{
    let mut sensor = Sht3x::new(i2c);
    sensor.soft_reset(delay)?;
    sensor.clear_status_and_wait(delay)?;
    sensor.measure(delay, Repeatability::High)
}

fn main() {}
