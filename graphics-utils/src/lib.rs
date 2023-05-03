#![no_std]

use embedded_graphics_core::{
    pixelcolor::BinaryColor,
    prelude::{Dimensions, DrawTarget},
    primitives::Rectangle,
    Pixel,
};

pub struct ColorInvertingOverlay<'a, T> {
    parent: &'a mut T,
    area: Rectangle,
}

impl<T> Dimensions for ColorInvertingOverlay<'_, T>
where
    T: Dimensions,
{
    fn bounding_box(&self) -> Rectangle {
        self.parent.bounding_box()
    }
}

impl<T> DrawTarget for ColorInvertingOverlay<'_, T>
where
    T: DrawTarget<Color = BinaryColor>,
{
    type Color = BinaryColor;
    type Error = T::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.parent
            .draw_iter(pixels.into_iter().map(|Pixel(pos, color)| {
                if self.area.contains(pos) {
                    Pixel(pos, color.invert())
                } else {
                    Pixel(pos, color)
                }
            }))
    }
}

pub trait BinaryColorDrawTargetExt: Sized {
    fn invert_area(&mut self, area: &Rectangle) -> ColorInvertingOverlay<'_, Self>;
}

impl<T> BinaryColorDrawTargetExt for T
where
    T: DrawTarget<Color = BinaryColor>,
{
    fn invert_area(&mut self, area: &Rectangle) -> ColorInvertingOverlay<'_, Self> {
        ColorInvertingOverlay {
            parent: self,
            area: *area,
        }
    }
}
