use core::mem::MaybeUninit;

#[global_allocator]
static ALLOCATOR: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();

pub fn init_heap() {
    const HEAP_SIZE: usize = 48 * 1024;
    static mut HEAP: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];

    use core::ptr::addr_of_mut;

    unsafe {
        let heap_size = HEAP.len();
        log::info!("Heap size: {heap_size}");
        ALLOCATOR.init(addr_of_mut!(HEAP).cast(), heap_size);
    }
}
