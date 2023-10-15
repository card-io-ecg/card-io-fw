use embedded_graphics::{
    image::{Image, ImageRaw},
    pixelcolor::BinaryColor,
    prelude::{Dimensions, DrawTarget, Point, Size},
    primitives::Rectangle,
    Drawable,
};
use embedded_layout::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WifiState {
    NotConnected,
    Connecting,
    Connected,
}

impl WifiState {
    #[rustfmt::skip]
    #[allow(clippy::unusual_byte_groupings)]
    const NOT_CONNECTED_DATA: ImageRaw<'static, BinaryColor> = ImageRaw::<BinaryColor>::new(&[
        0b00000000, 0b00000000,
        0b00111110, 0b00000000,
        0b01000001, 0b00000000,
        0b01000001, 0b00000000,
        0b00100010, 0b00000000,
        0b00100010, 0b00000000,
        0b00010100, 0b00000000,
        0b00001000, 0b00000000,
    ], 9);

    #[rustfmt::skip]
    #[allow(clippy::unusual_byte_groupings)]
    const CONNECTING_DATA: ImageRaw<'static, BinaryColor> = ImageRaw::<BinaryColor>::new(&[
        0b00000000, 0b00000000,
        0b00111110, 0b00000000,
        0b01000001, 0b00000000,
        0b01011101, 0b00000000,
        0b00100010, 0b00000000,
        0b00100010, 0b00000000,
        0b00010100, 0b00000000,
        0b00001000, 0b00000000,
    ], 9);

    #[rustfmt::skip]
    #[allow(clippy::unusual_byte_groupings)]
    const CONNECTED_DATA: ImageRaw<'static, BinaryColor> = ImageRaw::<BinaryColor>::new(&[
        0b00000000, 0b00000000,
        0b00111110, 0b00000000,
        0b01000001, 0b00000000,
        0b00011100, 0b00000000,
        0b00100010, 0b00000000,
        0b00001000, 0b00000000,
        0b00011100, 0b00000000,
        0b00001000, 0b00000000,
    ], 9);

    fn image(&self) -> ImageRaw<'static, BinaryColor> {
        match self {
            WifiState::NotConnected => Self::NOT_CONNECTED_DATA,
            WifiState::Connecting => Self::CONNECTING_DATA,
            WifiState::Connected => Self::CONNECTED_DATA,
        }
    }

    fn size(&self) -> Size {
        self.image().bounding_box().size
    }
}

#[derive(Clone, Copy)]
pub struct WifiStateView {
    pub data: Option<WifiState>,
    top_left: Point,
}

impl WifiStateView {
    #[inline]
    pub fn new(data: Option<impl Into<WifiState>>) -> Self {
        Self {
            data: data.map(Into::into),
            top_left: Point::zero(),
        }
    }

    #[inline]
    pub fn enabled(data: impl Into<WifiState>) -> Self {
        Self::new(Some(data.into()))
    }

    #[inline]
    pub fn disabled() -> Self {
        Self::new(None::<WifiState>)
    }

    #[inline]
    pub fn update(&mut self, connection_state: impl Into<WifiState>) {
        self.data = Some(connection_state.into());
    }
}

impl View for WifiStateView {
    #[inline]
    fn translate_impl(&mut self, by: Point) {
        self.top_left += by;
    }

    #[inline]
    fn bounds(&self) -> Rectangle {
        let size = self.data.map(|data| data.size()).unwrap_or_default();

        Rectangle::new(self.top_left, size)
    }
}

impl Drawable for WifiStateView {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        if let Some(data) = self.data {
            Image::new(&data.image(), self.top_left).draw(target)?;
        }

        Ok(())
    }
}
