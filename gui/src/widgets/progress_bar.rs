use core::num::NonZeroU32;

use embedded_graphics::{
    geometry::AnchorPoint,
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point, Size},
    primitives::{Primitive, PrimitiveStyle, Rectangle},
    Drawable,
};
use embedded_text::TextBox;

use crate::{
    screens::{CENTERED_TEXTBOX, NORMAL_TEXT},
    utils::BinaryColorDrawTargetExt,
};

// TODO: this is currently aligned to the bottom of the screen
pub struct ProgressBar<'a> {
    pub label: &'a str,
    pub progress: u32,
    pub max_progress: NonZeroU32,
}

impl Drawable for ProgressBar<'_> {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D: DrawTarget<Color = BinaryColor>>(&self, display: &mut D) -> Result<(), D::Error> {
        const BORDER_STYLE: PrimitiveStyle<BinaryColor> =
            PrimitiveStyle::with_stroke(BinaryColor::On, 1);
        const FILL_STYLE: PrimitiveStyle<BinaryColor> = PrimitiveStyle::with_fill(BinaryColor::On);

        let progress_border = Rectangle::new(Point::new(0, 51), Size::new(128, 13));
        let filler_area = progress_border.offset(-2); // 1px gap between border and fill

        let max_progress = self.max_progress.get();
        let empty_area_width =
            (self.progress.min(max_progress) * filler_area.size.width) / max_progress;

        // Progress filler - we could use the whole filler area but
        // let's resize to avoid unnecessary drawing
        let progress_filler = filler_area.resized(
            filler_area.size - Size::new(empty_area_width, 0),
            AnchorPoint::TopLeft,
        );

        progress_border.into_styled(BORDER_STYLE).draw(display)?;
        progress_filler.into_styled(FILL_STYLE).draw(display)?;

        // Invert the area on top of the progress filler so we can display text on both portions
        // of the progress bar with one draw call
        let mut draw_area = display.invert_area(&progress_filler);

        // using embedded-text because I'm lazy to position the label vertically
        TextBox::with_textbox_style(self.label, progress_border, NORMAL_TEXT, CENTERED_TEXTBOX)
            .set_vertical_offset(1) // Slight adjustment
            .draw(&mut draw_area)?;

        Ok(())
    }
}
