//! AES-128-GCM on esp-hal's `AesGcmContext`.
//!
//! The accelerators of the ESP32-C6, ESP32-S3 and ESP32-C5 have no GCM mode: esp-hal runs AES-CTR
//! on the accelerator, and GHASH on the CPU.

use embassy_crypto::{
    Error,
    driver::{Aes128Gcm, InOutBuf},
};
use esp_hal::aes::{
    self,
    gcm::{AesGcmContext, GcmError},
};

/// The key. An `AesGcmContext` cannot be cloned or shared, so each message runs on a new one,
/// which computes the hash subkey again in its first step.
#[derive(Clone)]
pub struct Context {
    key: [u8; 16],
}

impl Drop for Context {
    fn drop(&mut self) {
        for byte in &mut self.key {
            unsafe { core::ptr::write_volatile(byte, 0) };
        }
    }
}

/// esp-hal refuses an empty text, a text over 20,400 bytes and additional data over 4,080 bytes.
/// A TLS 1.3 record is never empty: it holds at least its content type.
fn input_error(_: aes::Error) -> Error {
    Error::InvalidInput
}

struct Driver;

impl Aes128Gcm for Driver {
    type Context = Context;

    fn init(key: &[u8; 16]) -> Context {
        Context { key: *key }
    }

    fn encrypt(
        ctx: &Context,
        nonce: &[u8; 12],
        aad: &[u8],
        buffer: InOutBuf<'_, '_, u8>,
        tag_out: &mut [u8; 16],
    ) -> Result<(), Error> {
        let text = buffer.into_out_with_copied_in();
        AesGcmContext::new(ctx.key)
            .encrypt_in_place(nonce, aad, text, tag_out)
            .map_err(input_error)?
            .wait_blocking()
            .map_err(|_| Error::HardwareError)
    }

    fn decrypt(
        ctx: &Context,
        nonce: &[u8; 12],
        aad: &[u8],
        buffer: InOutBuf<'_, '_, u8>,
        expected: &[u8; 16],
    ) -> Result<(), Error> {
        let text = buffer.into_out_with_copied_in();
        AesGcmContext::new(ctx.key)
            .decrypt_in_place(nonce, aad, text, expected)
            .map_err(input_error)?
            .wait_blocking()
            .map_err(|error| match error {
                GcmError::TagMismatch => Error::InvalidSignature,
                _ => Error::HardwareError,
            })
    }
}

embassy_crypto::aes128_gcm_impl!(Driver);
