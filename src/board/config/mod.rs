pub mod current;
pub mod v1;
pub mod v2;
pub mod v3;
pub mod v4;
pub mod v5;

pub mod types;

pub use current::Config;

use embedded_io_async::Read;
use norfs::storable::{LoadError, Loadable};

const CURRENT_VERSION: u8 = 5;

#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ConfigFile {
    V1(v1::Config),
    V2(v2::Config),
    V3(v3::Config),
    V4(v4::Config),
    V5(v5::Config),
    Current(Config),
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self::new(Config::default())
    }
}

impl ConfigFile {
    pub fn new(config: Config) -> Self {
        Self::Current(config)
    }

    /// Migrates config data to newest format.
    #[inline(never)]
    pub fn into_config(mut self) -> Config {
        if let Self::V1(config) = self {
            info!("Migrating config data to v2");
            self = Self::V2(v2::Config::from(config));
        }
        if let Self::V2(config) = self {
            info!("Migrating config data to v3");
            self = Self::V3(v3::Config::from(config));
        }
        if let Self::V3(config) = self {
            info!("Migrating config data to v4");
            self = Self::V4(v4::Config::from(config));
        }
        if let Self::V4(config) = self {
            info!("Migrating config data to v5");
            self = Self::V5(v5::Config::from(config));
        }
        if let Self::V5(config) = self {
            info!("Migrating config data to latest");
            self = Self::Current(Config::from(config));
        }

        match self {
            Self::Current(config) => config,
            _ => unreachable!(),
        }
    }
}

impl Loadable for ConfigFile {
    async fn load<R: Read>(reader: &mut R) -> Result<Self, LoadError<R::Error>> {
        let version = u8::load(reader).await?;
        info!("Loading config data with version {}", version + 1);
        let data = match version {
            0 => Self::V1(v1::Config::load(reader).await?),
            1 => Self::V2(v2::Config::load(reader).await?),
            2 => Self::V3(v3::Config::load(reader).await?),
            3 => Self::V4(v4::Config::load(reader).await?),
            4 => Self::V5(v5::Config::load(reader).await?),
            CURRENT_VERSION => Self::Current(Config::load(reader).await?),
            _ => return Err(LoadError::InvalidValue),
        };

        Ok(data)
    }
}
