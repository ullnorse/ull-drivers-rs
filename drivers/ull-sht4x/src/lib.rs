#![no_std]
#![doc = include_str!("../README.md")]

mod driver;
mod types;

pub use driver::Sht4x;
pub use types::{
    Address, DataWord, Error, FixedPointMeasurement, HeaterDuration, HeaterPower, Measurement,
    RawMeasurement, Repeatability, Result, crc8,
};

#[cfg(test)]
mod tests;
