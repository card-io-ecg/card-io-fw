use norfs::{
    medium::StorageMedium,
    reader::BoundReader,
    storable::{LoadError, Loadable, Storable},
    writer::BoundWriter,
    StorageError,
};

#[derive(Clone)]
pub struct WifiNetwork {
    pub ssid: heapless::String<32>,
    pub pass: heapless::String<64>,
}

impl Loadable for WifiNetwork {
    async fn load<M>(reader: &mut BoundReader<'_, M>) -> Result<Self, LoadError>
    where
        M: StorageMedium,
        [(); M::BLOCK_COUNT]: Sized,
    {
        let ssid = heapless::String::<32>::load(reader).await?;
        let pass = heapless::String::<64>::load(reader).await?;
        Ok(Self { ssid, pass })
    }
}

impl Storable for WifiNetwork {
    async fn store<M>(&self, writer: &mut BoundWriter<'_, M>) -> Result<(), StorageError>
    where
        M: StorageMedium,
        [(); M::BLOCK_COUNT]: Sized,
    {
        self.ssid.store(writer).await?;
        self.pass.store(writer).await?;
        Ok(())
    }
}
