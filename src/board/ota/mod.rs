use core::marker::PhantomData;

use crc::{Algorithm, Crc};
use embassy_futures::yield_now;
use macros::partition;
use norfs::medium::StorageMedium;
use norfs_driver::medium::MediumError;
use norfs_esp32s3::{InternalDriver, InternalPartition, SmallInternalDriver};

#[partition("otadata")]
pub struct OtaDataPartition;

#[partition("ota_0")]
pub struct Ota0Partition;

#[partition("ota_1")]
pub struct Ota1Partition;

#[derive(Clone, Copy, PartialEq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
enum OtaState {
    /// Monitor the first boot.
    /// In bootloader this state is changed to PendingVerify.
    New,

    /// First boot for this app was.
    /// If while the second boot this state is then it will be changed to ABORTED.
    PendingVerify,

    /// App was confirmed as workable. App can boot and work without limits.
    Valid,

    /// App was confirmed as non-workable. This app will not be selected to boot at all.
    Invalid,

    /// App could not confirm the workable or non-workable.
    /// In bootloader IMG_PENDING_VERIFY state will be changed to IMG_ABORTED.
    /// This app will not be selected to boot at all.
    Aborted,
}

impl TryFrom<u32> for OtaState {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0x0 => Ok(Self::New),
            0x1 => Ok(Self::PendingVerify),
            0x2 => Ok(Self::Valid),
            0x3 => Ok(Self::Invalid),
            0x4 => Ok(Self::Aborted),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct OtaHeader {
    ota_seq: u32,
    ota_state: Option<OtaState>,
    crc: u32,
}

impl OtaHeader {
    async fn read<P>(
        partition: &mut SmallInternalDriver<P>,
        slot: Slot,
    ) -> Result<Self, MediumError>
    where
        P: InternalPartition,
        SmallInternalDriver<P>: StorageMedium,
    {
        let mut buffer: [u8; 32] = [0; 32];
        partition.read(slot.block(), 0, &mut buffer[..]).await?;

        Ok(OtaHeader::from_buffer(buffer))
    }

    fn from_buffer(buffer: [u8; 32]) -> OtaHeader {
        OtaHeader {
            ota_seq: u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]),
            ota_state: OtaState::try_from(u32::from_le_bytes([
                buffer[24], buffer[25], buffer[26], buffer[27],
            ]))
            .ok(),
            crc: u32::from_le_bytes([buffer[28], buffer[29], buffer[30], buffer[31]]),
        }
    }

    fn into_buffer(self) -> [u8; 32] {
        let mut output = [0; 32];
        output[0..4].copy_from_slice(&self.ota_seq.to_le_bytes());
        output[24..28]
            .copy_from_slice(&self.ota_state.map_or(u32::MAX, |s| s as u32).to_le_bytes());
        output[28..32].copy_from_slice(&self.crc.to_le_bytes());
        output
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
enum Slot {
    Ota0,
    Ota1,
}

impl Slot {
    fn block(self) -> usize {
        match self {
            Slot::Ota0 => 0,
            Slot::Ota1 => 1,
        }
    }

    fn current(seq0: u32, seq1: u32) -> Option<Slot> {
        if seq0 == 0xffffffff && seq1 == 0xffffffff {
            None
        } else if seq0 == 0xffffffff {
            Some(Slot::Ota1)
        } else if seq1 == 0xffffffff {
            Some(Slot::Ota0)
        } else if seq0 > seq1 {
            Some(Slot::Ota0)
        } else {
            Some(Slot::Ota1)
        }
    }

    fn next(self) -> Slot {
        match self {
            Slot::Ota0 => Slot::Ota1,
            Slot::Ota1 => Slot::Ota0,
        }
    }
}

struct OtaData<P>
where
    P: InternalPartition,
{
    slot0: OtaHeader,
    slot1: OtaHeader,
    partition: SmallInternalDriver<P>,
}

impl<P> OtaData<P>
where
    P: InternalPartition,
    SmallInternalDriver<P>: StorageMedium,
{
    async fn read(mut partition: SmallInternalDriver<P>) -> Result<Self, MediumError> {
        Ok(Self {
            slot0: OtaHeader::read(&mut partition, Slot::Ota0).await?,
            slot1: OtaHeader::read(&mut partition, Slot::Ota1).await?,
            partition,
        })
    }

    fn app_slot(&self) -> Option<Slot> {
        Slot::current(self.slot0.ota_seq, self.slot1.ota_seq)
    }

    fn update_slot(&self) -> Slot {
        self.app_slot()
            .map(|slot| slot.next())
            .unwrap_or(Slot::Ota0)
    }

    fn next_sequence_count(&self) -> u32 {
        match (self.slot0.ota_seq, self.slot1.ota_seq) {
            (u32::MAX, u32::MAX) => 1,
            (u32::MAX, seq) | (seq, u32::MAX) => seq + 1,
            (seq0, seq1) => seq0.max(seq1) + 1,
        }
    }

    async fn erase(&mut self, slot: Slot) -> Result<(), MediumError> {
        self.partition.erase(slot.block()).await
    }

    async fn write(&mut self, slot: Slot, data: &[u8]) -> Result<(), MediumError> {
        self.partition.write(slot.block(), 0, data).await
    }
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum OtaError {
    Io,
}

impl From<MediumError> for OtaError {
    fn from(_: MediumError) -> Self {
        OtaError::Io
    }
}

pub struct OtaClient<D, P0, P1>
where
    D: InternalPartition,
    P0: InternalPartition,
    P1: InternalPartition,
{
    update_offset: usize,
    update_slot: Slot,
    ota_data: OtaData<D>,
    ota0: InternalDriver<P0>,
    ota1: InternalDriver<P1>,
    _marker: PhantomData<(D, P0, P1)>,
}

impl<D, P0, P1> OtaClient<D, P0, P1>
where
    D: InternalPartition,
    P0: InternalPartition,
    P1: InternalPartition,
    SmallInternalDriver<D>: StorageMedium,
    InternalDriver<P0>: StorageMedium,
    InternalDriver<P1>: StorageMedium,
{
    pub async fn initialize(data: D, ota0: P0, ota1: P1) -> Result<Self, OtaError> {
        let data = SmallInternalDriver::new(data);
        let ota_data = OtaData::read(data).await?;

        Ok(Self {
            update_offset: 0,
            update_slot: ota_data.update_slot(),
            ota_data,
            ota0: InternalDriver::new(ota0),
            ota1: InternalDriver::new(ota1),
            _marker: PhantomData,
        })
    }

    pub async fn erase(&mut self) -> Result<(), OtaError> {
        self.update_offset = 0;

        let count = match self.update_slot {
            Slot::Ota0 => InternalDriver::<P0>::BLOCK_COUNT,
            Slot::Ota1 => InternalDriver::<P1>::BLOCK_COUNT,
        };

        for block in 0..count {
            debug!("Erasing block {}/{}", block + 1, count);
            match self.update_slot {
                Slot::Ota0 => self.ota0.erase(block).await?,
                Slot::Ota1 => self.ota1.erase(block).await?,
            }
            yield_now().await;
        }

        Ok(())
    }

    pub async fn write(&mut self, buffer: &[u8]) -> Result<(), OtaError> {
        match self.update_slot {
            Slot::Ota0 => self.ota0.write(0, self.update_offset, buffer).await?,
            Slot::Ota1 => self.ota1.write(0, self.update_offset, buffer).await?,
        };
        self.update_offset += buffer.len();

        Ok(())
    }

    pub async fn activate(&mut self) -> Result<(), OtaError> {
        static CRC_ALGO: Algorithm<u32> = Algorithm {
            width: 32,
            poly: 0x04c11db7,
            init: 0,
            refin: true,
            refout: true,
            xorout: 0xffffffff,
            check: 0,
            residue: 0,
        };

        debug!("Activating {}", self.update_slot);

        self.ota_data.erase(self.update_slot).await?;

        let ota_seq = self.ota_data.next_sequence_count();

        let crc = Crc::<u32>::new(&CRC_ALGO);
        let mut digest = crc.digest();
        digest.update(&ota_seq.to_le_bytes());

        let header = OtaHeader {
            ota_seq,
            ota_state: Some(OtaState::Valid),
            crc: digest.finalize(),
        };

        self.ota_data
            .write(self.update_slot, &header.into_buffer())
            .await?;

        Ok(())
    }
}
