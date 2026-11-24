#![no_std]
#![doc = include_str!("../README.md")]

mod driver;
mod types;

pub use driver::Max17048;
pub use types::{
    Address, Config, Error, Result, Status, VoltageAlert, charge_rate_milli_percent_per_hour,
    soc_hundredths, vcell_microvolts,
};

#[cfg(test)]
mod tests;
