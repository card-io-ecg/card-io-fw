#![no_std]

#[macro_export]
macro_rules! trace {
    ($($args:tt)*) => {
        #[cfg(feature = "defmt")]
        defmt::trace!($($args)*);
        #[cfg(feature = "log")]
        log::trace!($($args)*);
    }
}

#[macro_export]
macro_rules! debug {
    ($($args:tt)*) => {
        #[cfg(feature = "defmt")]
        defmt::debug!($($args)*);
        #[cfg(feature = "log")]
        log::debug!($($args)*);
    }
}

#[macro_export]
macro_rules! info {
    ($($args:tt)*) => {
        #[cfg(feature = "defmt")]
        defmt::info!($($args)*);
        #[cfg(feature = "log")]
        log::info!($($args)*);
    }
}

#[macro_export]
macro_rules! warn {
    ($($args:tt)*) => {
        #[cfg(feature = "defmt")]
        defmt::warn!($($args)*);
        #[cfg(feature = "log")]
        log::warn!($($args)*);
    }
}

#[macro_export]
macro_rules! error {
    ($($args:tt)*) => {
        #[cfg(feature = "defmt")]
        defmt::error!($($args)*);
        #[cfg(feature = "log")]
        log::error!($($args)*);
    }
}
