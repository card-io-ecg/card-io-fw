use alloc::vec::Vec;
use ekv::{
    config::{MAX_PAGE_COUNT, PAGE_SIZE},
    flash::{Flash, PageID},
};
use esp_bootloader_esp_idf::partitions::{
    read_partition_table, DataPartitionSubType, PartitionType, PARTITION_TABLE_MAX_LEN,
};
use esp_storage::{FlashStorage, FlashStorageError};

pub struct PartitionFlash {
    storage: FlashStorage<'static>,
    offset: u32,
    pages: usize,
}

impl PartitionFlash {
    pub fn find(mut storage: FlashStorage<'static>, label: &str) -> Option<Self> {
        let mut buffer = Vec::new();
        if buffer.try_reserve_exact(PARTITION_TABLE_MAX_LEN).is_err() {
            error!("Out of memory while reading the partition table");
            return None;
        }
        buffer.resize(PARTITION_TABLE_MAX_LEN, 0);

        let table = match read_partition_table(&mut storage, &mut buffer) {
            Ok(table) => table,
            Err(e) => {
                error!("Failed to read the partition table: {:?}", e);
                return None;
            }
        };

        for entry in table.iter() {
            debug!(
                "Partition table entry: {} type {} subtype {} offset {} length {}",
                entry.label_as_str(),
                entry.raw_type(),
                entry.raw_subtype(),
                entry.offset(),
                entry.len()
            );
        }

        let Some(partition) = table.iter().find(|entry| {
            entry.label_as_str() == label
                && entry.partition_type() == PartitionType::Data(DataPartitionSubType::Undefined)
        }) else {
            error!("Partition not found: {}", label);
            return None;
        };

        let pages = (partition.len() as usize / PAGE_SIZE).min(MAX_PAGE_COUNT);
        info!("Partition {}: {} pages", label, pages);

        Some(Self {
            offset: partition.offset(),
            storage,
            pages,
        })
    }

    #[cfg(feature = "wifi")]
    pub fn storage_mut(&mut self) -> &mut FlashStorage<'static> {
        &mut self.storage
    }

    fn address(&self, page: PageID, offset: usize) -> u32 {
        self.offset + (page.index() * PAGE_SIZE + offset) as u32
    }
}

impl Flash for PartitionFlash {
    type Error = FlashStorageError;

    fn page_count(&self) -> usize {
        self.pages
    }

    async fn erase(&mut self, page: PageID) -> Result<(), Self::Error> {
        let from = self.address(page, 0);
        self.storage.erase(from, from + PAGE_SIZE as u32)
    }

    async fn read(
        &mut self,
        page: PageID,
        offset: usize,
        data: &mut [u8],
    ) -> Result<(), Self::Error> {
        let address = self.address(page, offset);
        self.storage.read_nor(address, data)
    }

    async fn write(&mut self, page: PageID, offset: usize, data: &[u8]) -> Result<(), Self::Error> {
        let address = self.address(page, offset);
        self.storage.write_nor(address, data)
    }
}
