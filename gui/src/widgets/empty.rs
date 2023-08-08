use embedded_graphics::{prelude::*, primitives::Rectangle};
use embedded_layout::{prelude::*, view_group::ViewGroup};

/// A [`ViewGroup`] that contains no [`View`] objects.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EmptyViewGroup;

/// A single instance of [`EmptyViewGroup`].
pub static mut EMPTY_VIEW_GROUP: EmptyViewGroup = EmptyViewGroup;

impl View for EmptyViewGroup {
    fn translate_impl(&mut self, _by: Point) {}

    fn bounds(&self) -> Rectangle {
        Rectangle::zero()
    }
}

impl ViewGroup for EmptyViewGroup {
    fn len(&self) -> usize {
        0
    }

    fn at(&self, _idx: usize) -> &dyn View {
        self
    }

    fn at_mut(&mut self, _idx: usize) -> &mut dyn View {
        self
    }
}
