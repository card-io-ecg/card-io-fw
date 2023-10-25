use embedded_graphics::{
    image::{Image, ImageRaw},
    pixelcolor::BinaryColor,
    prelude::{Dimensions, DrawTarget, Point, Size},
    primitives::Rectangle,
    Drawable,
};
use embedded_layout::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WifiClientState {
    NotConnected,
    Connecting,
    Connected,
}

impl WifiClientState {
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
            WifiClientState::NotConnected => Self::NOT_CONNECTED_DATA,
            WifiClientState::Connecting => Self::CONNECTING_DATA,
            WifiClientState::Connected => Self::CONNECTED_DATA,
        }
    }

    fn size(&self) -> Size {
        self.image().bounding_box().size
    }
}

#[derive(Clone, Copy)]
pub struct WifiClientStateView {
    pub data: Option<WifiClientState>,
    top_left: Point,
}

impl WifiClientStateView {
    #[inline]
    pub fn new(data: Option<impl Into<WifiClientState>>) -> Self {
        Self {
            data: data.map(Into::into),
            top_left: Point::zero(),
        }
    }

    #[inline]
    pub fn enabled(data: impl Into<WifiClientState>) -> Self {
        Self::new(Some(data.into()))
    }

    #[inline]
    pub fn disabled() -> Self {
        Self::new(None::<WifiClientState>)
    }

    #[inline]
    pub fn update(&mut self, connection_state: impl Into<WifiClientState>) {
        self.data = Some(connection_state.into());
    }
}

impl View for WifiClientStateView {
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

impl Drawable for WifiClientStateView {
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
