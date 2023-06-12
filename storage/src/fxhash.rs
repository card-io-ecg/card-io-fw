use core::{
    hash::{Hash, Hasher},
    ops::BitXor,
};

#[derive(Debug, Clone)]
pub struct FxHasher {
    hash: u32,
}

impl FxHasher {
    fn update(&mut self, word: u32) {
        self.hash = self
            .hash
            .rotate_left(5)
            .bitxor(word)
            .wrapping_mul(0x9e3779b9);
    }
}

impl Default for FxHasher {
    #[inline]
    fn default() -> FxHasher {
        FxHasher { hash: 0 }
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        while bytes.len() >= 4 {
            self.update(u32::from_le_bytes(bytes[..4].try_into().unwrap()));
            bytes = &bytes[4..];
        }

        if bytes.len() >= 2 {
            self.update(u16::from_le_bytes(bytes[..2].try_into().unwrap()) as u32);
            bytes = &bytes[2..];
        }

        if let Some(&byte) = bytes.first() {
            self.update(u32::from(byte));
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.update(u32::from(i));
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.update(u32::from(i));
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.update(i);
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.update(i as u32);
        self.update((i >> 32) as u32);
    }

    #[inline]
    fn finish(&self) -> u64 {
        u64::from(self.hash)
    }
}

/// A convenience function for when you need a quick hash.
#[inline]
pub fn hash32(v: &[u8]) -> u32 {
    let mut state = FxHasher::default();
    v.hash(&mut state);
    state.hash
}
