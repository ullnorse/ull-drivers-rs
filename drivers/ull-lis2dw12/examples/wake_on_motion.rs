use embedded_hal::delay::DelayNs;
use ull_lis2dw12::{
    Config, FullScale, InterruptMode, InterruptPinConfig, InterruptPolarity, Lis2dw12,
    OperatingMode, OutputDataRate, WakeUpConfig,
};

/// Configures wake-on-motion on INT1 for a board that needs an active-low pin.
#[allow(dead_code)]
fn arm_wake_on_motion<I2C, D>(
    i2c: I2C,
    delay: &mut D,
) -> Result<(), ull_lis2dw12::Error<I2C::Error>>
where
    I2C: embedded_hal::i2c::I2c,
    D: DelayNs,
{
    let mut sensor = Lis2dw12::new(i2c);
    sensor.init(delay)?;
    sensor.configure(&Config {
        odr: OutputDataRate::Hz25,
        mode: OperatingMode::LowPower,
        full_scale: FullScale::G2,
        ..Config::default()
    })?;
    sensor.set_interrupt_pin_config(&InterruptPinConfig {
        polarity: InterruptPolarity::ActiveLow,
        mode: InterruptMode::Latched,
        ..InterruptPinConfig::default()
    })?;
    // About 500 mg, held for two samples.
    let wake = WakeUpConfig::from_milli_g(FullScale::G2, 500, 2)
        .ok_or(ull_lis2dw12::Error::InvalidConfig)?;
    sensor.enable_wake_up_interrupt(&wake)
}

fn main() {}
