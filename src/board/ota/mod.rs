use macros::partition;
use norfs_esp32s3::InternalPartition;

pub struct PartitionTable;
impl InternalPartition for PartitionTable {
    const OFFSET: usize = 0x8000;
    const SIZE: usize = 0x1000;
}

#[partition("otadata")]
pub struct OtaDataPartition;

#[partition("ota_0")]
pub struct Ota0Partition;

#[partition("ota_1")]
pub struct Ota1Partition;
