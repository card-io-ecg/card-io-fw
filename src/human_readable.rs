use defmt::Format;
use embassy_time::Duration;
use ufmt::{uDisplay, uwrite};

const STEPS: &[(usize, &str)] = &[(1024, "k"), (1024 * 1024, "M"), (1024 * 1024 * 1024, "G")];

fn find_suffix(amount: usize) -> Option<(usize, usize, &'static str)> {
    for (limit, suffix) in STEPS.iter().cloned().rev() {
        if amount >= limit {
            let int = amount / limit;
            let frac = (amount % limit) / (limit / 10);
            return Some((int, frac, suffix));
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq)]
pub struct BinarySize(pub usize);

impl uDisplay for BinarySize {
    fn fmt<W>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error>
    where
        W: ufmt::uWrite + ?Sized,
    {
        if let Some((int, frac, suffix)) = find_suffix(self.0) {
            uwrite!(f, "{}.{}{}B", int, frac, suffix)
        } else {
            uwrite!(f, "{}B", self.0)
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct Throughput(pub usize, pub Duration);

impl Throughput {
    fn bytes_per_sec(self) -> usize {
        self.0 * 1000 / self.1.as_millis() as usize
    }
}

impl uDisplay for Throughput {
    fn fmt<W>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error>
    where
        W: ufmt::uWrite + ?Sized,
    {
        let bytes_per_sec = self.bytes_per_sec();
        if let Some((int, frac, suffix)) = find_suffix(bytes_per_sec) {
            uwrite!(f, "{}.{}{}B/s", int, frac, suffix)
        } else {
            uwrite!(f, "{}B/s", bytes_per_sec)
        }
    }
}

impl Format for Throughput {
    fn format(&self, fmt: defmt::Formatter) {
        let bytes_per_sec = self.bytes_per_sec();
        if let Some((int, frac, suffix)) = find_suffix(bytes_per_sec) {
            defmt::write!(fmt, "{}.{}{}B/s", int, frac, suffix)
        } else {
            defmt::write!(fmt, "{}B/s", bytes_per_sec)
        }
    }
}
