use embedded_graphics::{
    geometry::AnchorPoint,
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point, Size},
    primitives::{Primitive, PrimitiveStyle, Rectangle},
    Drawable,
};
use embedded_text::{
    alignment::{HorizontalAlignment, VerticalAlignment},
    style::{HeightMode, TextBoxStyleBuilder, VerticalOverdraw},
    TextBox,
};

use crate::utils::BinaryColorDrawTargetExt;

pub struct StartupScreen<'a> {
    pub label: &'a str,
    pub progress: u32,
    pub max_progress: u32,
}

impl Drawable for StartupScreen<'_> {
    type Color = BinaryColor;
    type Output = ();

    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        let progress_bar = Rectangle::new(Point::new(0, 51), Size::new(128, 13));
        let filler_area = progress_bar.offset(-2); // 1px gap between border and fill

        // Border
        progress_bar
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
            .draw(display)?;

        let filler_width = filler_area.size.width;
        let empty_area_width = (self.progress * filler_width) / self.max_progress;
        // remaining as in remaining time until measurement starts
        let remaining_width = filler_width - empty_area_width;

        // Progress filler - we could use the whole filler area but
        // let's resize to avoid unnecessary drawing
        let progress_filler = filler_area.resized(
            Size::new(remaining_width, filler_area.size.height),
            AnchorPoint::TopLeft,
        );

        progress_filler
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(display)?;

        // Invert the area on top of the progress filler so we can display text on both portions
        // of the progress bar with one draw call
        let mut draw_area = display.invert_area(&progress_filler);

        let textbox_style = TextBoxStyleBuilder::new()
            .height_mode(HeightMode::Exact(VerticalOverdraw::FullRowsOnly))
            .alignment(HorizontalAlignment::Center)
            .vertical_alignment(VerticalAlignment::Middle)
            .build();

        // using embedded-text because I'm lazy to position the label vertically
        TextBox::with_textbox_style(
            self.label,
            progress_bar,
            MonoTextStyleBuilder::new()
                .font(&FONT_6X10)
                .text_color(BinaryColor::On) // On on normally-Off background
                .build(),
            textbox_style,
        )
        .set_vertical_offset(1) // Slight adjustment
        .draw(&mut draw_area)?;

        Ok(())
    }
}
