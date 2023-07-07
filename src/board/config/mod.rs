pub mod current;
pub mod v1;

pub use current::Config;

use embedded_io::asynch::{Read, Write};
use norfs::storable::{LoadError, Loadable, Storable};

#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ConfigFile {
    V1(v1::Config),
    V2(Config),
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self::new(Config::default())
    }
}

impl ConfigFile {
    pub fn new(config: Config) -> Self {
        Self::V2(config)
    }

    /// Migrates config data to newest format.
    pub fn into_config(mut self) -> Config {
        if let Self::V1(config) = self {
            self = Self::V2(Config::from(config));
        }

        match self {
            Self::V2(config) => config,
            _ => unreachable!(),
        }
    }
}

impl Loadable for ConfigFile {
    async fn load<R: Read>(reader: &mut R) -> Result<Self, LoadError<R::Error>> {
        let data = match u8::load(reader).await? {
            0 => Self::V1(v1::Config::load(reader).await?),
            1 => Self::V2(Config::load(reader).await?),
            _ => return Err(LoadError::InvalidValue),
        };

        Ok(data)
    }
}

impl Storable for ConfigFile {
    async fn store<W: Write>(&self, writer: &mut W) -> Result<(), W::Error> {
        match self {
            Self::V1(config) => {
                0u8.store(writer).await?;
                config.store(writer).await?;
            }
            Self::V2(config) => {
                1u8.store(writer).await?;
                config.store(writer).await?;
            }
        }

        Ok(())
    }
}
