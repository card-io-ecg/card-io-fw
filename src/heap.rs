#[global_allocator]
static ALLOCATOR: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();

pub fn init_heap() {
    use core::ptr::{addr_of, addr_of_mut};

    extern "C" {
        static mut _heap_start: u32;
        static mut _heap_end: u32;
    }

    unsafe {
        let heap_start = addr_of!(_heap_start) as usize;
        let heap_end = addr_of!(_heap_end) as usize;

        let heap_size = heap_end - heap_start;
        log::info!("Heap size: {heap_size}");
        ALLOCATOR.init(addr_of_mut!(_heap_start).cast(), heap_size);
    }
}
