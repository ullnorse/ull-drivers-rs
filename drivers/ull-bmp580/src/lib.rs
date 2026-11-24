#![no_std]
#![doc = include_str!("../README.md")]

mod driver;
mod types;

pub use driver::Bmp580;
pub use types::{
    Address, Config, Error, FixedPointMeasurement, IirFilter, InterruptConfig, InterruptMode,
    InterruptOutput, InterruptPolarity, InterruptStatus, Measurement, OutputDataRate, Oversampling,
    PowerMode, RawMeasurement, Result,
};

#[cfg(test)]
mod tests;
