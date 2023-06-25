#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[allow(non_camel_case_types)]
pub struct i24([u8; 3]);

impl i24 {
    pub const ZERO: Self = Self([0; 3]);
    pub const MAX: Self = Self([0xff, 0xff, 0x7f]);
    pub const MIN: Self = Self([0x00, 0x00, 0x80]);

    pub const fn from_i32_lossy(value: i32) -> Self {
        let bytes = value.to_le_bytes();
        Self([bytes[0], bytes[1], bytes[2]])
    }

    pub const fn to_i32(self) -> i32 {
        i32::from_le_bytes([
            self.0[0],
            self.0[1],
            self.0[2],
            if self.0[2] & 0x80 == 0x80 { 0xff } else { 0x00 },
        ])
    }
}

#[cfg(test)]
mod test {
    use super::i24;

    #[test]
    fn conversion_returns_the_same_number() {
        for value in [0, 1, -1, 2, -2, 8_388_607, -8_388_608] {
            assert_eq!(i24::from_i32_lossy(value).to_i32(), value);
        }
    }
}
