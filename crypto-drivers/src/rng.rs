//! The random number generator.

use embassy_crypto::driver::Rng;

/// The hardware RNG mixes in RF noise while Wi-Fi runs, which makes it a true random source
/// (esp-hal `rng` docs). TLS and the device key draw from it only with Wi-Fi up.
struct Driver;

impl Rng for Driver {
    fn fill_bytes(buf: &mut [u8]) {
        esp_hal::rng::Rng::new().read(buf);
    }
}

embassy_crypto::rng_impl!(Driver);
