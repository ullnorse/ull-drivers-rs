use ull_max17048::{Max17048, VoltageAlert};

#[allow(dead_code)]
fn configure_and_clear<I2C>(i2c: I2C) -> Result<(), ull_max17048::Error<I2C::Error>>
where
    I2C: embedded_hal::i2c::I2c,
{
    let mut gauge = Max17048::new(i2c);
    gauge.set_low_soc_threshold(10)?;
    gauge.set_voltage_alert(VoltageAlert {
        min_mv: 3300,
        max_mv: 4300,
    })?;
    let _pending = gauge.clear_alert()?;
    Ok(())
}

fn main() {}
