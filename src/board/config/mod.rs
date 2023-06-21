pub mod current;

pub use current::Config;

use norfs::{
    medium::StorageMedium,
    reader::BoundReader,
    storable::{LoadError, Storable},
    writer::BoundWriter,
    StorageError,
};

#[derive(Clone, Copy)]
pub enum ConfigFile {
    V1(Config),
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self::new(Config::default())
    }
}

impl ConfigFile {
    pub fn new(config: Config) -> Self {
        Self::V1(config)
    }

    /// Migrates config data to newest format.
    pub fn into_config(self) -> Config {
        match self {
            Self::V1(config) => config,
        }
    }
}

impl Storable for ConfigFile {
    async fn load<M>(reader: &mut BoundReader<'_, M>) -> Result<Self, LoadError>
    where
        M: StorageMedium,
        [(); M::BLOCK_COUNT]: Sized,
    {
        let data = match u8::load(reader).await? {
            0 => Self::V1(Config::load(reader).await?),
            _ => return Err(LoadError::InvalidValue),
        };

        Ok(data)
    }

    async fn store<M>(&self, writer: &mut BoundWriter<'_, M>) -> Result<(), StorageError>
    where
        M: StorageMedium,
        [(); M::BLOCK_COUNT]: Sized,
    {
        match self {
            Self::V1(config) => {
                0u8.store(writer).await?;
                config.store(writer).await?;
            }
        }

        Ok(())
    }
}
