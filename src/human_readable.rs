use defmt::Format;
use embassy_time::Duration;
use ufmt::{uDebug, uDisplay, uwrite};

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

#[allow(clippy::collapsible_else_if)]
const fn digits(value: u32) -> usize {
    if value < 10_000 {
        if value < 100 {
            if value < 10 {
                1
            } else {
                2
            }
        } else {
            if value < 1_000 {
                3
            } else {
                4
            }
        }
    } else if value < 100_000_000 {
        if value < 1_000_000 {
            if value < 100_000 {
                5
            } else {
                6
            }
        } else {
            if value < 10_000_000 {
                7
            } else {
                8
            }
        }
    } else {
        if value < 1_000_000_000 {
            9
        } else {
            10
        }
    }
}

fn write_padding<W>(f: &mut ufmt::Formatter<'_, W>, len: usize, n: usize) -> Result<(), W::Error>
where
    W: ufmt::uWrite + ?Sized,
{
    const PADS: [&str; 5] = ["", " ", "  ", "   ", "    "];

    let mut pad_length = n.saturating_sub(len);

    while pad_length >= 5 {
        f.write_str("     ")?;
        pad_length -= 5;
    }
    f.write_str(PADS[pad_length])
}

#[derive(Clone, Copy, PartialEq)]
pub struct LeftPad(pub usize, pub i32);

impl uDisplay for LeftPad {
    fn fmt<W>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error>
    where
        W: ufmt::uWrite + ?Sized,
    {
        let len = if self.1 < 0 {
            digits(self.1.unsigned_abs()) + 1 // +1 for the minus sign
        } else {
            digits(self.1 as u32)
        };

        write_padding(f, len, self.0)?;

        uwrite!(f, "{}", self.1)
    }
}

struct LengthCounter(usize);
impl ufmt::uWrite for LengthCounter {
    type Error = core::convert::Infallible;

    fn write_str(&mut self, s: &str) -> Result<(), Self::Error> {
        self.0 += s.len();
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct LeftPadAny<D>(pub usize, pub D);

impl<D: uDisplay> uDisplay for LeftPadAny<D> {
    fn fmt<W>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error>
    where
        W: ufmt::uWrite + ?Sized,
    {
        let mut counter = LengthCounter(0);
        unwrap!(ufmt::uwrite!(&mut counter, "{}", self.1));

        write_padding(f, counter.0, self.0)?;

        ufmt::uwrite!(f, "{}", self.1)
    }
}

impl<D: uDebug> uDebug for LeftPadAny<D> {
    fn fmt<W>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error>
    where
        W: ufmt::uWrite + ?Sized,
    {
        let mut counter = LengthCounter(0);
        unwrap!(ufmt::uwrite!(&mut counter, "{:?}", self.1));

        write_padding(f, counter.0, self.0)?;

        ufmt::uwrite!(f, "{:?}", self.1)
    }
}

#[macro_export]
macro_rules! uformat {
    ($len:literal, $($arg:tt)*) => {
        {
            let mut s = heapless::String::<$len>::new();
            unwrap!(ufmt::uwrite!(&mut s, $($arg)*));
            s
        }
    }
}
