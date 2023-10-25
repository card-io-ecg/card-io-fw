use embedded_graphics::{
    image::{Image, ImageRaw},
    pixelcolor::BinaryColor,
    prelude::{Dimensions, DrawTarget, Point, Size},
    primitives::Rectangle,
    Drawable,
};
use embedded_layout::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WifiAccessPointState {
    NotConnected,
    Connected,
}

impl WifiAccessPointState {
    #[rustfmt::skip]
    #[allow(clippy::unusual_byte_groupings)]
    const NOT_CONNECTED_DATA: ImageRaw<'static, BinaryColor> = ImageRaw::<BinaryColor>::new(&[
        0b00000000, 0b00000000,
        0b01110000, 0b00000000,
        0b01001000, 0b00000000,
        0b01000100, 0b00000000,
        0b01000010, 0b00000000,
        0b01000001, 0b00000000,
        0b01000001, 0b00000000,
        0b01111111, 0b00000000,
    ], 9);

    #[rustfmt::skip]
    #[allow(clippy::unusual_byte_groupings)]
    const CONNECTED_DATA: ImageRaw<'static, BinaryColor> = ImageRaw::<BinaryColor>::new(&[
        0b00000000, 0b00000000,
        0b01100000, 0b00000000,
        0b00011000, 0b00000000,
        0b00000100, 0b00000000,
        0b01100010, 0b00000000,
        0b00010010, 0b00000000,
        0b00001001, 0b00000000,
        0b01001001, 0b00000000,
    ], 9);

    fn image(&self) -> ImageRaw<'static, BinaryColor> {
        match self {
            WifiAccessPointState::NotConnected => Self::NOT_CONNECTED_DATA,
            WifiAccessPointState::Connected => Self::CONNECTED_DATA,
        }
    }

    fn size(&self) -> Size {
        self.image().bounding_box().size
    }
}

#[derive(Clone, Copy)]
pub struct WifiAccessPointStateView {
    pub data: Option<WifiAccessPointState>,
    top_left: Point,
}

impl WifiAccessPointStateView {
    #[inline]
    pub fn new(data: Option<impl Into<WifiAccessPointState>>) -> Self {
        Self {
            data: data.map(Into::into),
            top_left: Point::zero(),
        }
    }

    #[inline]
    pub fn enabled(data: impl Into<WifiAccessPointState>) -> Self {
        Self::new(Some(data.into()))
    }

    #[inline]
    pub fn disabled() -> Self {
        Self::new(None::<WifiAccessPointState>)
    }

    #[inline]
    pub fn update(&mut self, connection_state: impl Into<WifiAccessPointState>) {
        self.data = Some(connection_state.into());
    }
}

impl View for WifiAccessPointStateView {
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

impl Drawable for WifiAccessPointStateView {
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
