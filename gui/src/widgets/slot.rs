use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point, Size},
    primitives::Rectangle,
    Drawable,
};
use embedded_layout::{view_group::ViewGroup, View};

use crate::widgets::empty::EMPTY_VIEW_GROUP;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Slot<T>
where
    T: View,
{
    Hidden(Point), // offset to apply when set to visible
    Visible(T),
}

impl<T> Default for Slot<T>
where
    T: View,
{
    fn default() -> Self {
        Self::Hidden(Point::zero())
    }
}

impl<T> Slot<T>
where
    T: View,
{
    pub fn empty() -> Self {
        Self::Hidden(Point::zero())
    }

    pub fn visible(view: T) -> Self {
        Self::Visible(view)
    }

    pub fn set_visible(&mut self, view: T) {
        *self = Self::Visible(view);
    }

    pub fn set_hidden(&mut self) {
        if let Self::Visible(view) = self {
            let offset = view.bounds().top_left;
            *self = Self::Hidden(offset);
        }
    }

    pub fn as_visible_mut(&mut self) -> Option<&mut T> {
        if let Self::Visible(view) = self {
            Some(view)
        } else {
            None
        }
    }
}

impl<T> View for Slot<T>
where
    T: View,
{
    fn translate_impl(&mut self, by: Point) {
        match self {
            Self::Hidden(offset) => *offset += by,
            Self::Visible(view) => view.translate_impl(by),
        }
    }

    fn bounds(&self) -> embedded_graphics::primitives::Rectangle {
        match self {
            Self::Hidden(point) => Rectangle::new(*point, Size::zero()),
            Self::Visible(view) => view.bounds(),
        }
    }
}

impl<T> ViewGroup for Slot<T>
where
    T: View,
{
    fn len(&self) -> usize {
        match self {
            Self::Hidden(_) => 0,
            Self::Visible(_) => 1,
        }
    }

    fn at(&self, _idx: usize) -> &dyn View {
        match self {
            Self::Hidden(_) => unsafe { &EMPTY_VIEW_GROUP },
            Self::Visible(view) => view,
        }
    }

    fn at_mut(&mut self, _idx: usize) -> &mut dyn View {
        match self {
            Self::Hidden(_) => unsafe { &mut EMPTY_VIEW_GROUP },
            Self::Visible(view) => view,
        }
    }
}

impl<T> Drawable for Slot<T>
where
    T: Drawable<Color = BinaryColor>,
    T: View,
{
    type Color = BinaryColor;
    type Output = ();

    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        if let Self::Visible(view) = self {
            view.draw(display)?;
        }

        Ok(())
    }
}
