//! embassy-crypto drivers on the esp-hal crypto backends.
//!
//! Each driver posts its work to an esp-hal work queue and blocks until it completes. [`start`]
//! starts the backends that serve the queues; a driver called before it blocks forever.

#![no_std]

mod aes_gcm;
#[cfg(feature = "ecc")]
mod p256;
mod rng;
mod sha256;

use esp_hal::{
    aes::dma::{AesDmaBackend, AesDmaChannel},
    peripherals::{AES, SHA},
    sha::ShaBackend,
};
use static_cell::StaticCell;

/// The crypto peripherals that the drivers take over.
pub struct Peripherals<'d, Dma: AesDmaChannel<'d>> {
    pub aes: AES<'d>,
    pub aes_dma: Dma,
    pub sha: SHA<'d>,
    #[cfg(feature = "ecc")]
    pub ecc: esp_hal::peripherals::ECC<'d>,
}

/// Starts the backends for the rest of the program.
///
/// # Panics
///
/// When called twice.
pub fn start(peripherals: Peripherals<'static, impl AesDmaChannel<'static>>) {
    static AES: StaticCell<AesDmaBackend<'static>> = StaticCell::new();
    static SHA: StaticCell<ShaBackend<'static>> = StaticCell::new();

    // Dropping a backend's driver stops the backend, and the backends serve until power-off.
    core::mem::forget(
        AES.init(AesDmaBackend::new(peripherals.aes, peripherals.aes_dma))
            .start(),
    );
    core::mem::forget(SHA.init(ShaBackend::new(peripherals.sha)).start());

    #[cfg(feature = "ecc")]
    {
        use esp_hal::ecc::EccBackend;

        static ECC: StaticCell<EccBackend<'static>> = StaticCell::new();
        let ecc = EccBackend::new(peripherals.ecc, Default::default())
            .unwrap_or_else(|_| panic!("the ECC backend did not start"));
        core::mem::forget(ECC.init(ecc).start());
    }
}
