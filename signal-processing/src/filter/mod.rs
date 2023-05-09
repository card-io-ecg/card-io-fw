use object_chain::{Chain, ChainElement, Link};

pub mod downsample;
pub mod fir;

pub trait Filter {
    fn update(&mut self, sample: f32) -> Option<f32>;
    fn clear(&mut self);
}

impl<F> Filter for Chain<F>
where
    F: Filter,
{
    fn update(&mut self, sample: f32) -> Option<f32> {
        self.object.update(sample)
    }

    fn clear(&mut self) {
        self.object.clear()
    }
}

impl<F, P> Filter for Link<F, P>
where
    F: Filter,
    P: ChainElement + Filter,
{
    fn update(&mut self, sample: f32) -> Option<f32> {
        let sample = self.parent.update(sample)?;
        self.object.update(sample)
    }

    fn clear(&mut self) {
        self.parent.clear();
        self.object.clear();
    }
}
