#![no_std]
#![doc = include_str!("../README.md")]

mod driver;
mod types;

pub use crate::driver::Ssd1681;
pub use crate::types::{
    BYTES_PER_ROW, Config, DeepSleepMode, Error, FRAME_LEN, FULL_REFRESH_TYPICAL_MS, HEIGHT,
    PARTIAL_REFRESH_TYPICAL_MS, PARTIALS_BEFORE_FULL, Pin, Result, WIDTH,
};

#[cfg(test)]
mod tests;
