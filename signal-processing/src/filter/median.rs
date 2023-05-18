use crate::sliding::SlidingWindow;

use super::Filter;

#[derive(Default)]
pub struct MedianFilter<const N: usize> {
    buffer: SlidingWindow<N>,
}

impl<const N: usize> MedianFilter<N> {
    pub fn new() -> Self {
        Self {
            buffer: SlidingWindow::new(),
        }
    }

    fn nth(data: &mut [f32; N], n: usize) -> f32 {
        for i in 0..(n + 1) {
            let min_value = data[i];

            for j in i + 1..data.len() {
                if data[j] < min_value {
                    data.swap(i, j);
                }
            }
        }
        data[n]
    }
}

impl<const N: usize> Filter for MedianFilter<N> {
    fn clear(&mut self) {
        self.buffer.clear();
    }

    fn update(&mut self, sample: f32) -> Option<f32> {
        self.buffer.push(sample);

        if self.buffer.is_full() {
            let mut iter = self.buffer.iter();

            let mut copy: [f32; N] = [0.0; N];
            copy.fill_with(|| iter.next().unwrap());
            debug_assert!(iter.next().is_none());

            Some(Self::nth(&mut copy, N / 2))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test() {
        let mut filter: MedianFilter<5> = MedianFilter::new();
        filter.update(0.0);
        filter.update(1.0);
        filter.update(2.0);
        filter.update(3.0);
        assert_eq!(2.0, filter.update(4.0).unwrap());
        assert_eq!(2.0, filter.update(1.0).unwrap());
        assert_eq!(2.0, filter.update(2.0).unwrap());
        assert_eq!(3.0, filter.update(5.0).unwrap());
    }
}
