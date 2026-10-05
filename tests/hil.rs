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
    use alloc::{boxed::Box, format, string::String, vec::Vec};

    use config_site::data::network::WifiNetwork;
    use config_types::{types::DisplayBrightness, Config};
    use defmt::info;
    use device_auth_firmware::{parse_code, registration, request_token};
    use ekv::{config::MAX_VALUE_SIZE, flash::Flash, Database};
    use embassy_sync_06::blocking_mutex::raw::NoopRawMutex;
    use embassy_time::Instant;
    use esp_bootloader_esp_idf::partitions::{read_partition_table, PARTITION_TABLE_MAX_LEN};
    use esp_hal::{clock::CpuClock, peripherals::FLASH, timer::systimer::SystemTimer};
    use esp_storage::FlashStorage;
    use network_services::pairing::{Name, SigningKey};

    use crate::board::storage::{self, measurement_buffer, FileSystem};

    const HIL_PARTITION: &str = "hil";

    // The values of `NEUTRAL_REGISTRATION` and `NEUTRAL_TOKEN` in `network_services::pairing::test`.
    const NEUTRAL_REGISTRATION: &str = "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCIsImp3ayI6eyJrdHkiOiJFQyIsImNydiI6IlAtMjU2IiwieCI6ImJfQTdsSkpCemgydDFEVVo1cFlPQ29XMEdtbWdYREtCQTZvcnpoV1V5aFkiLCJ5IjoiUEU5MU9sV19BZHhUOXNDd3gtN25pMERHXzMwbHFXNGlncm1KenZjY0ZFbyJ9fQ.eyJuYW1lIjoiY2FyZGlvLWExYjJjM2Q0ZTVmNiIsImNvZGVfbWFjIjoiNEItbG5GU0xGbDJTNmNXV0J4SWRKZmFlNVFlVEE2UHVncHZBM0xTc3cyNCJ9.zBs3SmYaY6Tm-UNyIbsz37r74B648gG4ZMDXPj4Pz-4lXajle9vvMa9TtD6uxztsRvvt_tvfyGia1QjVx-EIjg";
    const NEUTRAL_TOKEN: &str = "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJjYXJkaW8tYTFiMmMzZDRlNWY2IiwianRpIjoiNDIiLCJodG0iOiJERUxFVEUiLCJodHUiOiIvYXBpL2RldmljZXMvY2FyZGlvLWExYjJjM2Q0ZTVmNiJ9.XDluue20BrBIDZT1EPSERhzOZNhQkwyVjwCf7CbOZJUxZAumPtpWlJdNWbNttzhxBSBipO0gxqaMQmlduJfXzg";

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

    fn neutral_key() -> SigningKey {
        SigningKey::from_bytes(&[1; 32].into()).expect("the neutral key is a valid scalar")
    }

    /// Remounts the store with a `device_key` record of any content. `FileSystem` saves only
    /// valid keys, so a second database writes the record through the flash lock.
    async fn remount_with_key_record(fs: FileSystem, record: &[u8]) -> FileSystem {
        drop(fs);

        {
            let mut flash = storage::lock_flash().await.expect("no flash lock");
            let database = Box::new(Database::<_, NoopRawMutex>::new(
                &mut *flash,
                ekv::Config::default(),
            ));
            database
                .mount()
                .await
                .expect("planting database did not mount");
            let mut tx = database.write_transaction().await;
            tx.write(b"device_key", record).await.expect("write failed");
            tx.commit().await.expect("commit failed");
        }

        FileSystem::mount().await.expect("storage did not remount")
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

        let mut payload = measurement_buffer().unwrap();
        assert_eq!(fs.load_oldest_measurement(&mut payload).await, Ok(Some(1)));
        assert_eq!(payload, pattern(100));

        assert_eq!(fs.delete_oldest_measurement().await, Ok(()));
        assert_eq!(fs.measurement_count().await, Ok(1));
        assert_eq!(fs.load_oldest_measurement(&mut payload).await, Ok(Some(2)));
        assert_eq!(payload, pattern(200));

        assert_eq!(fs.delete_oldest_measurement().await, Ok(()));
        assert_eq!(fs.has_measurements().await, Ok(false));
        assert_eq!(fs.load_oldest_measurement(&mut payload).await, Ok(None));
    }

    #[test]
    async fn stored_measurements_survive_a_remount(board: Board) {
        let mut fs = mount_erased(board.flash).await;
        assert_eq!(fs.store_measurement(3, &pattern(500)).await, Ok(()));
        drop(fs);

        let mut fs = FileSystem::mount().await.expect("storage did not remount");
        assert_eq!(fs.measurement_count().await, Ok(1));
        let mut payload = measurement_buffer().unwrap();
        assert_eq!(fs.load_oldest_measurement(&mut payload).await, Ok(Some(3)));
        assert_eq!(payload, pattern(500));
    }

    #[test]
    async fn a_measurement_of_the_largest_size_round_trips(board: Board) {
        let mut fs = mount_erased(board.flash).await;
        let size = MAX_VALUE_SIZE;

        assert_eq!(fs.store_measurement(1, &pattern(size)).await, Ok(()));

        let mut payload = measurement_buffer().unwrap();
        assert_eq!(fs.load_oldest_measurement(&mut payload).await, Ok(Some(1)));
        assert_eq!(payload.len(), size);
        assert!(payload
            .iter()
            .enumerate()
            .all(|(index, byte)| *byte == pattern_byte(index)));
    }

    #[test]
    async fn a_saved_key_loads_back_after_a_remount(board: Board) {
        let mut fs = mount_erased(board.flash).await;
        assert!(fs.load_key().await.is_none());
        assert_eq!(fs.save_key(&neutral_key()).await, Ok(()));
        drop(fs);

        let mut fs = FileSystem::mount().await.expect("storage did not remount");
        let loaded = fs.load_key().await.expect("the key did not load");
        assert_eq!(loaded.to_bytes(), neutral_key().to_bytes());
    }

    #[test]
    async fn a_deleted_key_loads_as_none(board: Board) {
        let mut fs = mount_erased(board.flash).await;
        assert_eq!(fs.save_key(&neutral_key()).await, Ok(()));
        assert_eq!(fs.delete_key().await, Ok(()));
        assert!(fs.load_key().await.is_none());
        drop(fs);

        let mut fs = FileSystem::mount().await.expect("storage did not remount");
        assert!(fs.load_key().await.is_none());
    }

    #[test]
    async fn a_key_record_of_the_wrong_length_loads_as_none(board: Board) {
        let fs = mount_erased(board.flash).await;

        let mut fs = remount_with_key_record(fs, &[1; 31]).await;
        assert!(fs.load_key().await.is_none());

        let mut fs = remount_with_key_record(fs, &[1; 33]).await;
        assert!(fs.load_key().await.is_none());

        let mut fs = remount_with_key_record(fs, &[]).await;
        assert!(fs.load_key().await.is_none());
    }

    #[test]
    async fn a_key_record_that_is_not_a_valid_scalar_loads_as_none(board: Board) {
        let fs = mount_erased(board.flash).await;

        let mut fs = remount_with_key_record(fs, &[0; 32]).await;
        assert!(fs.load_key().await.is_none());

        let mut fs = remount_with_key_record(fs, &[0xFF; 32]).await;
        assert!(fs.load_key().await.is_none());
    }

    #[test]
    async fn a_valid_key_record_written_around_the_filesystem_loads(board: Board) {
        let fs = mount_erased(board.flash).await;

        let mut fs = remount_with_key_record(fs, &neutral_key().to_bytes()).await;
        let loaded = fs.load_key().await.expect("the key did not load");
        assert_eq!(loaded.to_bytes(), neutral_key().to_bytes());
    }

    #[test]
    async fn the_chip_signs_the_neutral_inputs_as_the_host_does() {
        let key = neutral_key();
        let name = Name::from_mac([0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6]);
        let code = parse_code("7KQ2MX9D4F").expect("the neutral code does not parse");

        let started = Instant::now();
        let mut body = String::new();
        registration(&key, name.as_str(), &code, &mut body).expect("registration failed");
        info!(
            "registration signing took {} ms",
            started.elapsed().as_millis()
        );
        assert_eq!(body, NEUTRAL_REGISTRATION);

        let path = format!("/api/devices/{}", name.as_str());
        let started = Instant::now();
        let mut token = String::new();
        request_token(&key, name.as_str(), 42, "DELETE", &path, &mut token)
            .expect("request token failed");
        info!(
            "request token signing took {} ms",
            started.elapsed().as_millis()
        );
        assert_eq!(token, NEUTRAL_TOKEN);
    }
}
