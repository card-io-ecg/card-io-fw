use crate::medium::WriteGranularity;

use super::maybe_with_critical_section;

pub(super) const WRITE_GRANULARITY: WriteGranularity = WriteGranularity::Bit;
pub(super) const BLOCK_SIZE: usize = 65536;
pub(super) const PAGE_SIZE: usize = 256;

macro_rules! rom_fn {
    (fn $name:ident($($arg:tt: $ty:ty),*) -> $retval:ty = $addr:expr) => {
        #[inline(always)]
        #[allow(unused)]
        #[link_section = ".rwtext"]
        pub(crate) fn $name($($arg:$ty),*) -> i32 {
            maybe_with_critical_section(|| unsafe {
                let rom_fn: unsafe extern "C" fn($($arg: $ty),*) -> $retval =
                    core::mem::transmute($addr as usize);
                    rom_fn($($arg),*)
            })
        }
    };

    ($(fn $name:ident($($arg:tt: $ty:ty),*) -> $retval:ty = $addr:expr),+) => {
        $(
            rom_fn!(fn $name($($arg: $ty),*) -> $retval = $addr);
        )+
    };
}

rom_fn!(
    fn esp_rom_spiflash_read(src_addr: u32, data: *mut u32, len: u32) -> i32 = 0x40000a20,
    fn esp_rom_spiflash_unlock() -> i32 = 0x40000a2c,
    fn esp_rom_spiflash_erase_block(block_number: u32) -> i32 = 0x40000a08,
    fn esp_rom_spiflash_write(dest_addr: u32, data: *const u32, len: u32) -> i32 = 0x40000a14,
    fn esp_rom_spiflash_read_user_cmd(status: *mut u32, cmd: u8) -> i32 = 0x40000a5c
);
