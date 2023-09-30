use ufmt::{uDisplay, uwrite};

pub struct BinarySize(pub usize);

impl uDisplay for BinarySize {
    fn fmt<W>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error>
    where
        W: ufmt::uWrite + ?Sized,
    {
        const SUFFIXES: &[&str] = &["kB", "MB", "GB"];
        const SIZES: &[usize] = &[1024, 1024 * 1024, 1024 * 1024 * 1024];

        for (size, suffix) in SIZES.iter().cloned().zip(SUFFIXES.iter().cloned()).rev() {
            if self.0 >= size {
                let int = self.0 / size;
                let frac = (self.0 % size) / (size / 10);
                uwrite!(f, "{}.{}{}", int, frac, suffix)?;
                return Ok(());
            }
        }

        uwrite!(f, "{}B", self.0)
    }
}
