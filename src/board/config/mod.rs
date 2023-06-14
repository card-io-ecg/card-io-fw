pub mod current;

pub use current::Config;

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
    pub fn parse(_buffer: &[u8]) -> Result<Self, ()> {
        Ok(Self::V1(Config::default()))
    }

    /// Migrates config data to newest format.
    pub fn into_config(self) -> Config {
        match self {
            Self::V1(config) => config,
        }
    }
}
