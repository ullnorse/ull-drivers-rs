#![no_std]
#![doc = include_str!("../README.md")]

mod driver;
mod size;
mod types;

pub use crate::driver::{
    BufferedGraphicsMode, RawMode, ScrollActive, ScrollInactive, ScrollRestoreRequired, Ssd1306,
    StateChangeError,
};
pub use crate::size::{DisplaySize, DisplaySize96x16, DisplaySize128x32, DisplaySize128x64};
pub use crate::types::{
    Address, ComScanDirection, Config, DisplayLine, DisplayOffset, Error, InitError,
    InvalidArgument, Orientation, Page, PageRange, PowerSource, Result, Rotation, RowCount,
    ScrollDirection, ScrollFrameInterval, SegmentRemap, VerticalScrollArea,
};

#[cfg(test)]
mod tests;
