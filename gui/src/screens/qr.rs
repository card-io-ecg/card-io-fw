use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point, Size},
    primitives::{Primitive, PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
    Drawable,
};
use qrcodegen_no_heap::{QrCode, QrCodeEcc, Version};
use ufmt::uwrite;

use crate::screens::{message::MessageScreen, NORMAL_TEXT};

pub struct QrCodeScreen<'a> {
    pub message: &'a str,
    pub countdown: Option<usize>,
    pub invert: bool,
}

impl<'a> QrCodeScreen<'a> {
    pub const fn new(message: &'a str) -> Self {
        Self {
            message,
            countdown: None,
            invert: false,
        }
    }
}

impl Drawable for QrCodeScreen<'_> {
    type Color = BinaryColor;
    type Output = ();

    #[inline]
    fn draw<D>(&self, display: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        const MAX_VER: u8 = 3;
        let mut buffer = [0; Version::new(MAX_VER).buffer_len()];
        let mut tempbuffer = [0; Version::new(MAX_VER).buffer_len()];

        let data = QrCode::encode_text(
            self.message,
            &mut tempbuffer[..],
            &mut buffer[..],
            QrCodeEcc::Medium,
            Version::MIN,
            Version::new(MAX_VER),
            None,
            true,
        );

        match data {
            Ok(qr) => {
                let min_qr_side = qr.size();

                let size = display.bounding_box().size;
                let scale = size.width.min(size.height) as i32 / min_qr_side;

                let qr_side = min_qr_side * scale;

                let x_offset = (size.width as i32 - qr_side) / 2;
                let y_offset = (size.height as i32 - qr_side) / 2;

                for y in -scale..qr.size() + scale {
                    for x in -scale..qr.size() + scale {
                        if self.invert ^ !qr.get_module(x, y) {
                            continue;
                        }

                        Rectangle::new(
                            Point::new(x_offset + x * scale, y_offset + y * scale),
                            Size::new(scale as u32, scale as u32),
                        )
                        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                        .draw(display)?;
                    }
                }
            }
            Err(_) => MessageScreen {
                message: "Failed to render QR code",
            }
            .draw(display)?,
        }

        if let Some(countdown) = self.countdown {
            let status_loc = display.bounding_box().top_left;

            let mut str_buffer = heapless::String::<16>::new();
            unwrap!(uwrite!(&mut str_buffer, "{}s", countdown));
            Text::with_baseline(&str_buffer, status_loc, NORMAL_TEXT, Baseline::Top)
                .draw(display)?;
        }

        Ok(())
    }
}
