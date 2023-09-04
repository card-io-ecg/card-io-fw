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

pub fn interpolate(value: u32, x_min: u32, x_max: u32, y_min: u32, y_max: u32) -> u32 {
    let x_range = x_max - x_min;
    let y_range = y_max - y_min;

    if x_range == 0 {
        y_min
    } else {
        let x = value - x_min;
        let y = x * y_range / x_range;

        y + y_min
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn interpolate_basic() {
        assert_eq!(interpolate(0, 0, 100, 0, 100), 0);
        assert_eq!(interpolate(50, 0, 100, 0, 100), 50);
        assert_eq!(interpolate(100, 0, 100, 0, 100), 100);
        assert_eq!(interpolate(100, 0, 10, 0, 100), 1000);
    }
}
