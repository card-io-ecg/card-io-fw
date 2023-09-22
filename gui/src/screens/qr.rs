use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point, Size},
    primitives::{Primitive, PrimitiveStyle, Rectangle},
    Drawable,
};
use qrcodegen_no_heap::{QrCode, QrCodeEcc, Version};

use crate::screens::message::MessageScreen;

pub struct QrCodeScreen<'a> {
    pub message: &'a str,
}

impl<'a> QrCodeScreen<'a> {
    pub const fn new(message: &'a str) -> Self {
        Self { message }
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
                let min_qr_side = qr.size() as i32;

                let size = display.bounding_box().size;
                let scale = size.width.min(size.height) as i32 / min_qr_side;

                let qr_side = min_qr_side * scale;

                let x_offset = (size.width as i32 - qr_side) / 2;
                let y_offset = (size.height as i32 - qr_side) / 2;

                for y in 0..qr.size() {
                    for x in 0..qr.size() {
                        if !qr.get_module(x, y) {
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

        Ok(())
    }
}
