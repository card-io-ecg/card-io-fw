//! Simple linear interpolation

pub struct Interval {
    min: f32,
    width: f32,
}

impl Interval {
    pub fn new(min: f32, max: f32) -> Self {
        Self {
            min,
            width: max - min,
        }
    }
}

pub struct Lerp {
    pub from: Interval,
    pub to: Interval,
}

impl Lerp {
    pub fn map(&self, value: f32) -> f32 {
        if self.from.width == 0.0 {
            self.to.min
        } else {
            (value - self.from.min) * (self.to.width / self.from.width) + self.to.min
        }
    }
}
