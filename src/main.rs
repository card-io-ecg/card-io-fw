#![no_std]
#![no_main]
#![feature(allocator_api)] // Box::try_new
#![feature(impl_trait_in_assoc_type)]

extern crate alloc;

// MUST be the first module
mod fmt;

#[cfg(feature = "esp-println")]
use esp_println as _;

use alloc::{boxed::Box, rc::Rc};
use embassy_executor::Spawner;
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    mutex::{Mutex, MutexGuard},
};
use embassy_time::{Duration, Timer};
use signal_processing::compressing_buffer::CompressingBuffer;
use static_cell::StaticCell;

#[cfg(feature = "wifi")]
use network_services::pairing::Pairing;

#[cfg(feature = "wifi")]
use crate::states::{
    firmware_update::firmware_update, throughput::throughput,
    upload_or_store_measurement::upload_stored_measurements,
};
use crate::{
    board::{
        initialized::{Context, InnerContext},
        startup::StartupResources,
        storage::FileSystem,
    },
    states::{
        charging::charging,
        display_serial::display_serial,
        init::initialize,
        measure::{measure, ECG_BUFFER_SIZE},
        menu::{display_menu_screen, AppMenu},
        upload_or_store_measurement::upload_or_store_measurement,
        MESSAGE_DURATION,
    },
};
use config_types::Config;

use esp_hal::{interrupt::Priority, rtc_cntl::WakeLock};
use esp_rtos::embassy::InterruptExecutor;

esp_bootloader_esp_idf::esp_app_desc!();

mod board;
pub mod human_readable;
mod states;
mod task_control;
mod timeout;

pub struct SerialNumber;

impl SerialNumber {
    pub fn bytes() -> [u8; 6] {
        let mut mac_bytes = [0u8; 6];
        let mac = esp_hal::efuse::base_mac_address();
        mac_bytes.copy_from_slice(mac.as_bytes());
        mac_bytes
    }
}

impl ufmt::uDisplay for SerialNumber {
    fn fmt<W>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error>
    where
        W: ufmt::uWrite + ?Sized,
    {
        for byte in Self::bytes() {
            ufmt::uwrite!(f, "{:X}", byte)?;
        }
        Ok(())
    }
}

impl core::fmt::Display for SerialNumber {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let serial = uformat!(12, "{}", self);
        f.write_str(&serial)
    }
}

pub type Shared<T> = Rc<Mutex<NoopRawMutex, T>>;
pub type SharedGuard<'a, T> = MutexGuard<'a, NoopRawMutex, T>;

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum AppState {
    PreInitialize,
    Initialize,
    Measure,
    Charging,
    Menu(AppMenu),
    DisplaySerial,
    #[cfg(feature = "wifi")]
    FirmwareUpdate,
    #[cfg(feature = "wifi")]
    Throughput,
    Shutdown,
    #[cfg(feature = "wifi")]
    UploadStored(AppMenu),
    UploadOrStore(Box<CompressingBuffer<ECG_BUFFER_SIZE>>),
}

fn log_heap(stage: &str) {
    info!(
        "Heap {}: {} bytes used, {} bytes free",
        stage,
        esp_alloc::HEAP.used(),
        esp_alloc::HEAP.free()
    );
}

async fn load_config(storage: Option<&mut FileSystem>) -> &'static mut Config {
    static CONFIG: StaticCell<Config> = StaticCell::new();

    let config = match storage {
        Some(storage) => {
            if let Ok(count) = storage.measurement_count().await {
                info!("Storage: {} saved measurements", count);
            }
            storage.load_config().await
        }
        None => {
            warn!("Storage unavailable. Using default config");
            Config::default()
        }
    };

    CONFIG.init(config)
}

#[cfg(feature = "wifi")]
async fn load_pairing(storage: Option<&mut FileSystem>) -> Pairing {
    let key = match storage {
        Some(storage) => storage.load_key().await,
        None => None,
    };

    let pairing = Pairing::new(key);
    if pairing.paired() {
        info!("Device is paired");
    } else {
        info!("Device is unpaired");
    }
    pairing
}

#[esp_rtos::main]
async fn main(_spawner: Spawner) {
    #[cfg(all(feature = "rtt", feature = "defmt"))]
    rtt_target::rtt_init_defmt!();

    let wake_lock = Some(WakeLock::new());

    const RECLAIMED_SIZE: usize = const {
        let range = esp_metadata_generated::memory_range!("DRAM2_UNINIT");
        range.end - range.start
    };

    // ECG_BUFFER_SIZE must fit in one of these regions
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: RECLAIMED_SIZE);
    esp_alloc::heap_allocator!(size: 96 * 1024);

    let resources = StartupResources::initialize().await;

    static INTERRUPT_EXECUTOR: StaticCell<InterruptExecutor<2>> = StaticCell::new();
    let interrupt_executor =
        INTERRUPT_EXECUTOR.init(InterruptExecutor::new(resources.software_interrupt2));

    info!("Hardware version: {}", env!("HW_VERSION"));

    board::storage::init(resources.flash, "storage");

    let mut storage = FileSystem::mount().await;
    log_heap("after mount");
    let config = load_config(storage.as_mut()).await;
    log_heap("after load_config");
    #[cfg(feature = "wifi")]
    let pairing = load_pairing(storage.as_mut()).await;

    // We're boxing Context because we will need to move out of it during shutdown.
    let mut board = Box::new(Context {
        // If the device is awake, the display should be enabled.
        frontend: resources.frontend,
        storage,
        #[cfg(feature = "wifi")]
        pairing,
        inner: InnerContext {
            display: resources.display,
            high_prio_spawner: interrupt_executor.start(Priority::Priority2),
            battery_monitor: resources.battery_monitor,
            #[cfg(feature = "wifi")]
            wifi: {
                use board::wifi::WifiDriver;
                static WIFI: StaticCell<WifiDriver> = StaticCell::new();
                WIFI.init(WifiDriver::new(resources.wifi))
            },
            config,
            config_changed: true,
            sta_work_available: None,
            message_displayed_at: None,
            wake_lock,
        },
    });

    unwrap!(board.inner.display.enable().await.ok());

    board.apply_hw_config_changes().await;
    board.config_changed = false;

    let mut state = AppState::PreInitialize;

    loop {
        info!("New app state: {:?}", state);
        state = match state {
            AppState::PreInitialize => {
                if board.battery_monitor.is_plugged() {
                    AppState::Charging
                } else {
                    AppState::Initialize
                }
            }
            AppState::Initialize => initialize(&mut board).await,
            AppState::Charging => charging(&mut board).await,
            AppState::Measure => measure(&mut board).await,
            AppState::Menu(menu) => display_menu_screen(menu, &mut board).await,
            AppState::DisplaySerial => display_serial(&mut board).await,
            #[cfg(feature = "wifi")]
            AppState::FirmwareUpdate => firmware_update(&mut board).await,
            #[cfg(feature = "wifi")]
            AppState::Throughput => throughput(&mut board).await,
            #[cfg(feature = "wifi")]
            AppState::UploadStored(next_state) => {
                upload_stored_measurements(&mut board, AppState::Menu(next_state)).await
            }
            AppState::UploadOrStore(buffer) => {
                upload_or_store_measurement(&mut board, buffer, AppState::Shutdown).await
            }
            AppState::Shutdown => break,
        };

        board.wait_for_message(MESSAGE_DURATION).await;
    }

    board.inner.display.shut_down();

    board.frontend.wait_for_release().await;
    Timer::after(Duration::from_millis(100)).await;

    let is_charging = board.inner.battery_monitor.is_plugged();
    board.inner.battery_monitor.stop().await;

    board::enter_sleep(is_charging);
    // Shouldn't reach this. If we do, we just exit the task, which means the executor
    // will have nothing else to do. Not ideal, but again, we shouldn't reach this.
}
