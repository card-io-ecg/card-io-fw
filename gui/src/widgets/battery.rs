use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point, Size},
    primitives::{Primitive, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle, StrokeAlignment},
    Drawable,
};
use embedded_layout::prelude::*;

const WALL_THICKNESS: u32 = 3;
const BAR_SPACE: u32 = 2;
const BAR_SIZE: Size = Size::new(5, 10);
const MAX_BARS: u32 = 4;
const POSITIVE_WIDTH: u32 = 3;

pub struct Battery {
    n_bars: u32,
    top_left: Point,
}

impl Battery {
    pub fn new(n_bars: u32, top_left: Point) -> Self {
        Self {
            n_bars: n_bars.min(MAX_BARS),
            top_left,
        }
    }

    const fn body_size() -> Size {
        Size::new(
            MAX_BARS * BAR_SIZE.width + (MAX_BARS + 1) * BAR_SPACE + 2 * WALL_THICKNESS,
            BAR_SIZE.height + 2 * BAR_SPACE + 2 * WALL_THICKNESS,
        )
    }
}

impl Drawable for Battery {
    type Color = BinaryColor;
    type Output = ();

    #[inline]
    fn draw<D: DrawTarget<Color = BinaryColor>>(&self, display: &mut D) -> Result<(), D::Error> {
        let body_style = PrimitiveStyleBuilder::new()
            .stroke_color(BinaryColor::On)
            .stroke_width(WALL_THICKNESS)
            .stroke_alignment(StrokeAlignment::Inside)
            .build();
        let box_style = PrimitiveStyle::with_fill(BinaryColor::On);

        let body = Rectangle::new(self.top_left, Self::body_size());
        let positive = Rectangle::new(Point::zero(), Size::new(POSITIVE_WIDTH, BAR_SIZE.height))
            .align_to(&body, horizontal::LeftToRight, vertical::Center);
        let bar = Rectangle::new(Point::zero(), BAR_SIZE);

        body.into_styled(body_style).draw(display)?;
        positive.into_styled(box_style).draw(display)?;

        let mut anchor = Rectangle::new(
            self.top_left,
            Size::new(WALL_THICKNESS, Self::body_size().height),
        );

        for _ in 0..self.n_bars {
            let rect = bar
                .align_to(&anchor, horizontal::LeftToRight, vertical::Center)
                .translate(Point::new(BAR_SPACE as i32, 0));

            rect.into_styled(box_style).draw(display)?;

            anchor = rect;
        }

        Ok(())
    }
}

impl View for Battery {
    #[inline]
    fn translate_impl(&mut self, by: Point) {
        self.top_left += by;
    }

    #[inline]
    fn bounds(&self) -> Rectangle {
        Rectangle::new(
            self.top_left,
            Self::body_size() + Size::new(POSITIVE_WIDTH, 0),
        )
    }
}
