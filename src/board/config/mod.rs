use serde::{Deserialize, Serialize};

pub mod current;

pub use current::Config;

#[derive(Serialize, Deserialize)]
pub enum ConfigFile {
    V1(Config),
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self::new(Config::default())
    }
}

impl ConfigFile {
    pub const MAX_CONFIG_SIZE: usize = 1 + Config::MAX_CONFIG_SIZE;

    pub fn new(config: Config) -> Self {
        Self::V1(config)
    }

    /// Parses config data.
    pub fn parse(buffer: &[u8]) -> Result<Self, ()> {
        postcard::from_bytes(buffer).map_err(|_| ())
    }

    /// Migrates config data to newest format.
    pub fn into_config(self) -> Config {
        match self {
            Self::V1(config) => config,
        }
    }

    /// Serializes config data.
    pub fn into_vec(self) -> heapless::Vec<u8, { Self::MAX_CONFIG_SIZE }> {
        postcard::to_vec(&self).unwrap()
    }
}
