use embedded_graphics::{
    image::{Image, ImageRaw},
    pixelcolor::BinaryColor,
    prelude::{Dimensions, DrawTarget, DrawTargetExt, Point, Size},
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
        0b00000000, 0b00000_000,
        0b00011111, 0b11000_000,
        0b01100000, 0b00110_000,
        0b00100000, 0b00100_000,
        0b00010000, 0b01000_000,
        0b00001000, 0b10000_000,
        0b00000101, 0b00000_000,
        0b00000010, 0b00000_000,
        0b00000000, 0b00000_000,
    ], 13);

    #[rustfmt::skip]
    #[allow(clippy::unusual_byte_groupings)]
    const CONNECTING_DATA: ImageRaw<'static, BinaryColor> = ImageRaw::<BinaryColor>::new(&[
        0b00000000, 0b00000_000,
        0b00011111, 0b11000_000,
        0b01100000, 0b00110_000,
        0b00100001, 0b00100_000,
        0b00011111, 0b11000_000,
        0b00001111, 0b10000_000,
        0b00000111, 0b00000_000,
        0b00000010, 0b00000_000,
        0b00000000, 0b00000_000,
    ], 13);

    #[rustfmt::skip]
    #[allow(clippy::unusual_byte_groupings)]
    const CONNECTED_DATA: ImageRaw<'static, BinaryColor> = ImageRaw::<BinaryColor>::new(&[
        0b00000000, 0b00000_000,
        0b00011111, 0b11000_000,
        0b01100000, 0b00110_000,
        0b00000111, 0b00000_000,
        0b00011000, 0b11000_000,
        0b00000010, 0b00000_000,
        0b00000111, 0b00000_000,
        0b00000010, 0b00000_000,
        0b00000000, 0b00000_000,
    ], 13);

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

    fn draw<D: DrawTarget<Color = BinaryColor>>(
        &self,
        target: &mut D,
        top_left: Point,
    ) -> Result<Point, D::Error> {
        Image::new(&self.image(), top_left).draw(target)?;

        Ok(top_left)
    }
}

#[derive(Clone, Copy)]
pub struct WifiStateView {
    pub data: Option<WifiState>,
    top_left: Point,
}

impl WifiStateView {
    pub fn new(data: Option<impl Into<WifiState>>) -> Self {
        Self {
            data: data.map(Into::into),
            top_left: Point::zero(),
        }
    }

    pub fn enabled(data: impl Into<WifiState>) -> Self {
        Self::new(Some(data.into()))
    }

    pub fn disabled() -> Self {
        Self::new(None::<WifiState>)
    }

    pub fn update(&mut self, connection_state: impl Into<WifiState>) {
        self.data = Some(connection_state.into());
    }
}

impl View for WifiStateView {
    fn translate_impl(&mut self, by: Point) {
        self.top_left += by;
    }

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
            let mut cropped = target.cropped(&self.bounds());
            data.draw(&mut cropped, self.top_left)?;
        }

        Ok(())
    }
}
