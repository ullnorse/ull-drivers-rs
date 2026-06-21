pub(crate) mod private {
    use super::{DisplaySize96x16, DisplaySize128x32, DisplaySize128x64};

    pub trait Sealed {}

    pub trait DisplaySizePrivate {
        fn make_buffer() -> <Self as super::DisplaySize>::Buffer
        where
            Self: super::DisplaySize;
        fn multiplex() -> u8;
        fn com_pins() -> u8;
    }

    impl Sealed for DisplaySize128x64 {}
    impl Sealed for DisplaySize128x32 {}
    impl Sealed for DisplaySize96x16 {}
}

/// Typed SSD1306 panel geometry.
pub trait DisplaySize: Copy + private::Sealed + private::DisplaySizePrivate {
    /// Panel width in pixels.
    const WIDTH: u8;
    /// Panel height in pixels.
    const HEIGHT: u8;
    /// Framebuffer storage type.
    type Buffer: AsRef<[u8]> + AsMut<[u8]>;
}

/// 128x64 SSD1306 panel.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct DisplaySize128x64;

impl DisplaySize for DisplaySize128x64 {
    const WIDTH: u8 = 128;
    const HEIGHT: u8 = 64;
    type Buffer = [u8; 1024];
}

impl private::DisplaySizePrivate for DisplaySize128x64 {
    fn make_buffer() -> <Self as DisplaySize>::Buffer {
        [0; 1024]
    }

    fn multiplex() -> u8 {
        0x3F
    }

    fn com_pins() -> u8 {
        0x12
    }
}

/// 128x32 SSD1306 panel.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct DisplaySize128x32;

impl DisplaySize for DisplaySize128x32 {
    const WIDTH: u8 = 128;
    const HEIGHT: u8 = 32;
    type Buffer = [u8; 512];
}

impl private::DisplaySizePrivate for DisplaySize128x32 {
    fn make_buffer() -> <Self as DisplaySize>::Buffer {
        [0; 512]
    }

    fn multiplex() -> u8 {
        0x1F
    }

    fn com_pins() -> u8 {
        0x02
    }
}

/// 96x16 SSD1306 panel.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct DisplaySize96x16;

impl DisplaySize for DisplaySize96x16 {
    const WIDTH: u8 = 96;
    const HEIGHT: u8 = 16;
    type Buffer = [u8; 192];
}

impl private::DisplaySizePrivate for DisplaySize96x16 {
    fn make_buffer() -> <Self as DisplaySize>::Buffer {
        [0; 192]
    }

    fn multiplex() -> u8 {
        0x0F
    }

    fn com_pins() -> u8 {
        0x02
    }
}
