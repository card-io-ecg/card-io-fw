#![no_std]

#[cfg(feature = "defmt")]
pub use defmt;
#[cfg(not(feature = "defmt"))]
pub use noop as defmt;

#[cfg(feature = "log")]
pub use log;
#[cfg(not(feature = "log"))]
pub use noop as log;

#[cfg(any(not(feature = "defmt"), not(feature = "log")))]
pub mod noop {
    #[macro_export]
    macro_rules! noop {
        ($($args:tt)*) => {};
    }

    pub use noop as trace;
    pub use noop as debug;
    pub use noop as info;
    pub use noop as warn;
    pub use noop as error;
}

#[macro_export]
macro_rules! trace {
    ($($args:tt)*) => {{
        $crate::defmt::trace!($($args)*);
        $crate::log::trace!($($args)*);
    }}
}

#[macro_export]
macro_rules! debug {
    ($($args:tt)*) => {{
        $crate::defmt::debug!($($args)*);
        $crate::log::debug!($($args)*);
    }}
}

#[macro_export]
macro_rules! info {
    ($($args:tt)*) => {{
        $crate::defmt::info!($($args)*);
        $crate::log::info!($($args)*);
    }}
}

#[macro_export]
macro_rules! warn {
    ($($args:tt)*) => {{
        $crate::defmt::warn!($($args)*);
        $crate::log::warn!($($args)*);
    }}
}

#[macro_export]
macro_rules! error {
    ($($args:tt)*) => {{
        $crate::defmt::error!($($args)*);
        $crate::log::error!($($args)*);
    }}
}
