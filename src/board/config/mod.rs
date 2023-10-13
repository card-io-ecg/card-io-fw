pub mod current;
pub mod v1;
pub mod v2;
pub mod v3;
pub mod v4;

pub mod types;

pub use current::Config;

use embedded_io::asynch::Read;
use norfs::storable::{LoadError, Loadable};

const CURRENT_VERSION: u8 = 4;

#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ConfigFile {
    V1(v1::Config),
    V2(v2::Config),
    V3(v3::Config),
    V4(v4::Config),
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
            self = Self::V2(v2::Config::from(config));
        }
        if let Self::V2(config) = self {
            self = Self::V3(v3::Config::from(config));
        }
        if let Self::V3(config) = self {
            self = Self::V4(v4::Config::from(config));
        }
        if let Self::V4(config) = self {
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
        let data = match u8::load(reader).await? {
            0 => Self::V1(v1::Config::load(reader).await?),
            1 => Self::V2(v2::Config::load(reader).await?),
            2 => Self::V3(v3::Config::load(reader).await?),
            3 => Self::V4(v4::Config::load(reader).await?),
            CURRENT_VERSION => Self::Current(Config::load(reader).await?),
            _ => return Err(LoadError::InvalidValue),
        };

        Ok(data)
    }
}
