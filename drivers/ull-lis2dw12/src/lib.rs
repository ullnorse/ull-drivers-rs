#![no_std]
#![doc = include_str!("../README.md")]

mod driver;
mod types;

pub use driver::Lis2dw12;
pub use types::{
    Address, AllInterruptSource, BandwidthFilter, Config, Error, FullScale, InterruptMode,
    InterruptOutput, InterruptPinConfig, InterruptPolarity, LowPowerMode, OperatingMode,
    OutputDataRate, RawAcceleration, Resolution, Result, Status, WakeUpConfig, WakeUpSource,
};

#[cfg(test)]
mod tests;
