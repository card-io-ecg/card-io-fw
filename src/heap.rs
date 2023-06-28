#[global_allocator]
static ALLOCATOR: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();

pub fn init_heap() {
    static mut HEAP: [u8; 32 * 1024] = [0; 32 * 1024];

    use core::ptr::addr_of_mut;

    unsafe {
        let heap_size = HEAP.len();
        log::info!("Heap size: {heap_size}");
        ALLOCATOR.init(addr_of_mut!(HEAP).cast(), heap_size);
    }
}
