use core::{mem::MaybeUninit, ptr::addr_of_mut};

#[global_allocator]
pub static ALLOCATOR: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();

pub fn init_heap() {
    const HEAP_SIZE: usize = (48 + 96) * 1024;
    static mut HEAP: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];

    unsafe {
        let heap_size = HEAP.len();
        info!("Heap size: {}", heap_size);
        ALLOCATOR.init(addr_of_mut!(HEAP).cast(), heap_size);
    }
}
