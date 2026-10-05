//! Hardware-in-the-loop tests of the storage. `cargo xtask hil` flashes them through the probe-rs
//! runner, which resets the chip before each test. They use the `hil` partition and never touch
//! `storage`.

#![no_std]
#![no_main]
#![feature(allocator_api)] // Box::try_new
#![feature(impl_trait_in_assoc_type)]

extern crate alloc;

mod board;

esp_bootloader_esp_idf::esp_app_desc!();

#[embedded_test::setup]
fn setup() {
    rtt_target::rtt_init_defmt!();
}

#[embedded_test::tests(executor = esp_rtos::embassy::Executor::new())]
mod tests {
    use alloc::vec::Vec;

    use config_site::data::network::WifiNetwork;
    use config_types::{types::DisplayBrightness, Config};
    use ekv::{config::MAX_VALUE_SIZE, flash::Flash};
    use esp_bootloader_esp_idf::partitions::{read_partition_table, PARTITION_TABLE_MAX_LEN};
    use esp_hal::{clock::CpuClock, peripherals::FLASH, timer::systimer::SystemTimer};
    use esp_storage::FlashStorage;

    use crate::board::storage::{self, FileSystem};

    const HIL_PARTITION: &str = "hil";

    struct Board {
        flash: FLASH<'static>,
    }

    #[init]
    async fn init() -> Board {
        let cpu_clock = cfg_select! {
            feature = "esp32s3" => CpuClock::_160MHz,
            _ => CpuClock::max(),
        };
        let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(cpu_clock));

        esp_alloc::heap_allocator!(size: 192 * 1024);

        let systimer = SystemTimer::new(peripherals.SYSTIMER);
        esp_rtos::start(systimer.alarm0, peripherals.FROM_CPU_INTR0);

        Board {
            flash: peripherals.FLASH,
        }
    }

    /// Erases the whole `hil` partition, so the store has to be formatted by the first mount.
    fn erase_partition(flash: FLASH<'_>) {
        let mut storage = FlashStorage::new(flash);

        let mut buffer = Vec::new();
        buffer.resize(PARTITION_TABLE_MAX_LEN, 0);
        let table = read_partition_table(&mut storage, &mut buffer).expect("no partition table");
        let partition = table
            .iter()
            .find(|entry| entry.label_as_str() == HIL_PARTITION)
            .expect("no `hil` partition");

        let from = partition.offset();
        storage
            .erase(from, from + partition.len())
            .expect("erase failed");
    }

    async fn mount_erased(mut flash: FLASH<'static>) -> FileSystem {
        erase_partition(flash.reborrow());
        storage::init(flash, HIL_PARTITION);
        FileSystem::mount().await.expect("storage did not mount")
    }

    fn changed_config() -> Config {
        let defaults = Config::default();

        let mut config = Config {
            display_brightness: DisplayBrightness::Brightest,
            use_external_clock: !defaults.use_external_clock,
            ..defaults
        };
        config.backend_url.clear();
        config
            .backend_url
            .push_str("http://localhost:8080")
            .unwrap();
        config
            .known_networks
            .push(WifiNetwork {
                ssid: "home".try_into().unwrap(),
                pass: "secret".try_into().unwrap(),
            })
            .unwrap();
        config
    }

    fn pattern_byte(index: usize) -> u8 {
        (index % 251) as u8
    }

    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(pattern_byte).collect()
    }

    #[test]
    async fn the_flash_lock_is_available_only_once_the_store_is_initialised(board: Board) {
        assert!(storage::lock_flash().await.is_none());

        let _fs = mount_erased(board.flash).await;

        let flash = storage::lock_flash().await.expect("no flash lock");
        assert!(flash.page_count() > 0);
    }

    #[test]
    async fn an_erased_partition_is_formatted_and_loads_the_default_config(board: Board) {
        let mut fs = mount_erased(board.flash).await;

        assert_eq!(fs.load_config().await, Config::default());
        assert_eq!(fs.measurement_count().await, Ok(0));
        assert_eq!(fs.has_measurements().await, Ok(false));
    }

    #[test]
    async fn a_saved_config_loads_back_after_a_remount(board: Board) {
        let mut fs = mount_erased(board.flash).await;
        let config = changed_config();
        assert_eq!(fs.save_config(&config).await, Ok(()));
        drop(fs);

        let mut fs = FileSystem::mount().await.expect("storage did not remount");
        assert_eq!(fs.load_config().await, config);
    }

    #[test]
    async fn formatting_empties_the_store_and_resets_the_config(board: Board) {
        let mut fs = mount_erased(board.flash).await;
        assert_eq!(fs.save_config(&changed_config()).await, Ok(()));
        assert_eq!(fs.store_measurement(1, &pattern(100)).await, Ok(()));
        drop(fs);

        FileSystem::format().await;

        let mut fs = FileSystem::mount().await.expect("storage did not remount");
        assert_eq!(fs.load_config().await, Config::default());
        assert_eq!(fs.measurement_count().await, Ok(0));
    }

    #[test]
    async fn measurements_come_back_oldest_first_and_deleting_removes_the_oldest(board: Board) {
        let mut fs = mount_erased(board.flash).await;
        assert_eq!(fs.store_measurement(1, &pattern(100)).await, Ok(()));
        assert_eq!(fs.store_measurement(2, &pattern(200)).await, Ok(()));
        assert_eq!(fs.measurement_count().await, Ok(2));
        assert_eq!(fs.has_measurements().await, Ok(true));

        // A loaded measurement keeps the capacity of the largest possible one, so each is dropped
        // before the next load.
        {
            let oldest = fs.load_oldest_measurement().await.unwrap().unwrap();
            assert_eq!(oldest.version, 1);
            assert_eq!(oldest.payload, pattern(100));
        }

        assert_eq!(fs.delete_oldest_measurement().await, Ok(()));
        assert_eq!(fs.measurement_count().await, Ok(1));
        {
            let next = fs.load_oldest_measurement().await.unwrap().unwrap();
            assert_eq!(next.version, 2);
            assert_eq!(next.payload, pattern(200));
        }

        assert_eq!(fs.delete_oldest_measurement().await, Ok(()));
        assert_eq!(fs.has_measurements().await, Ok(false));
        assert!(fs.load_oldest_measurement().await.unwrap().is_none());
    }

    #[test]
    async fn stored_measurements_survive_a_remount(board: Board) {
        let mut fs = mount_erased(board.flash).await;
        assert_eq!(fs.store_measurement(3, &pattern(500)).await, Ok(()));
        drop(fs);

        let mut fs = FileSystem::mount().await.expect("storage did not remount");
        assert_eq!(fs.measurement_count().await, Ok(1));
        let stored = fs.load_oldest_measurement().await.unwrap().unwrap();
        assert_eq!(stored.version, 3);
        assert_eq!(stored.payload, pattern(500));
    }

    #[test]
    async fn a_measurement_of_the_largest_size_round_trips(board: Board) {
        let mut fs = mount_erased(board.flash).await;
        let size = MAX_VALUE_SIZE;

        assert_eq!(fs.store_measurement(1, &pattern(size)).await, Ok(()));

        let stored = fs.load_oldest_measurement().await.unwrap().unwrap();
        assert_eq!(stored.payload.len(), size);
        assert!(stored
            .payload
            .iter()
            .enumerate()
            .all(|(index, byte)| *byte == pattern_byte(index)));
    }
}
