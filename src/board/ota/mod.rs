use alloc::boxed::Box;
use embedded_storage::nor_flash::NorFlash;
use esp_bootloader_esp_idf::{
    ota::OtaImageState, ota_updater::OtaUpdater, partitions::PARTITION_TABLE_MAX_LEN,
};
use esp_storage::FlashStorage;

const SECTOR_SIZE: usize = 4096;
const ERASE_STEP: u32 = 64 * 1024;

// `write_nor` copies through a 4096 byte stack buffer unless the data is 4-byte aligned.
#[repr(align(4))]
struct Staging([u8; SECTOR_SIZE]);

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum OtaError {
    Io,
}

impl From<esp_bootloader_esp_idf::partitions::Error> for OtaError {
    fn from(_: esp_bootloader_esp_idf::partitions::Error) -> Self {
        OtaError::Io
    }
}

pub struct OtaClient<'a> {
    updater: OtaUpdater<'a, 'static>,
    staging: Box<Staging>,
    staged: usize,
    offset: u32,
}

impl<'a> OtaClient<'a> {
    pub fn initialize(
        flash: &'a mut FlashStorage<'static>,
        partition_table: &'a mut [u8; PARTITION_TABLE_MAX_LEN],
    ) -> Result<Self, OtaError> {
        let staging = Box::try_new(Staging([0xFF; SECTOR_SIZE])).map_err(|_| OtaError::Io)?;

        Ok(Self {
            updater: OtaUpdater::new(flash, partition_table)?,
            staging,
            staged: 0,
            offset: 0,
        })
    }

    pub async fn erase(&mut self) -> Result<(), OtaError> {
        self.offset = 0;
        self.staged = 0;

        let (mut region, slot) = self.updater.next_partition()?;
        let size = region.partition_size() as u32;
        debug!("Erasing {:?}", slot);

        let mut from = 0;
        while from < size {
            let to = (from + ERASE_STEP).min(size);
            region.erase(from, to)?;
            from = to;

            embassy_futures::yield_now().await;
        }

        Ok(())
    }

    pub fn write(&mut self, mut data: &[u8]) -> Result<(), OtaError> {
        while !data.is_empty() {
            let (chunk, rest) = data.split_at(data.len().min(SECTOR_SIZE - self.staged));
            self.staging.0[self.staged..][..chunk.len()].copy_from_slice(chunk);
            self.staged += chunk.len();
            data = rest;

            if self.staged == SECTOR_SIZE {
                self.flush()?;
            }
        }

        Ok(())
    }

    pub fn activate(&mut self) -> Result<(), OtaError> {
        if self.staged > 0 {
            self.flush()?;
        }

        self.updater.activate_next_partition()?;
        self.updater.set_current_ota_state(OtaImageState::Valid)?;

        Ok(())
    }

    fn flush(&mut self) -> Result<(), OtaError> {
        let len = self.staged.next_multiple_of(4);
        self.staging.0[self.staged..len].fill(0xFF);

        let (mut region, _) = self.updater.next_partition()?;
        region
            .as_nor_flash()?
            .write(self.offset, &self.staging.0[..len])?;

        self.offset += len as u32;
        self.staged = 0;

        Ok(())
    }
}
