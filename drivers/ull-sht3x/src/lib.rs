#![no_std]
#![doc = include_str!("../README.md")]

mod driver;
mod types;

pub use driver::{ArtMode, PeriodicMode, Sht3x, SingleShotMode};
pub use types::{
    Address, DataWord, Error, FixedPointMeasurement, Measurement, PeriodicRate, RawMeasurement,
    Repeatability, Result, Status, crc8,
};

#[cfg(test)]
mod tests;
