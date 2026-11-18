//! SHA-256 and HMAC-SHA256 on the SHA accelerator.

use embassy_crypto::driver::{HmacSha256, Sha256};
use esp_hal::sha::Sha256Context;

const BLOCK: usize = 64;

/// A hash in progress. The accelerator saves and restores its state for each context, so any
/// number of them can be in progress, and a clone continues on its own.
#[derive(Clone)]
pub struct Context(Sha256Context);

// `Sha256Context` keeps the work queue's raw pointers, which are only used while a call borrows
// the context.
unsafe impl Send for Context {}
unsafe impl Sync for Context {}

impl Context {
    fn new() -> Self {
        Self(Sha256Context::new())
    }

    fn update(&mut self, data: &[u8]) {
        self.0.update(data).wait_blocking();
    }

    fn finalize(mut self, out: &mut [u8; 32]) {
        self.0.finalize(out).wait_blocking();
    }
}

struct ShaDriver;

impl Sha256 for ShaDriver {
    type Context = Context;

    fn init() -> Context {
        Context::new()
    }

    fn update(ctx: &mut Context, data: &[u8]) {
        ctx.update(data);
    }

    fn finalize(ctx: Context, out: &mut [u8; 32]) {
        ctx.finalize(out);
    }
}

embassy_crypto::sha256_impl!(ShaDriver);

/// HMAC (RFC 2104): the inner hash in progress, and the key padded for the outer hash.
#[derive(Clone)]
pub struct HmacContext {
    inner: Context,
    outer_key: [u8; BLOCK],
}

impl Drop for HmacContext {
    fn drop(&mut self) {
        // The padded key is the key.
        for byte in &mut self.outer_key {
            unsafe { core::ptr::write_volatile(byte, 0) };
        }
    }
}

struct HmacDriver;

impl HmacSha256 for HmacDriver {
    type Context = HmacContext;

    fn init(key: &[u8]) -> HmacContext {
        let mut block = [0u8; BLOCK];
        if key.len() > BLOCK {
            let mut hashed = [0u8; 32];
            let mut hash = Context::new();
            hash.update(key);
            hash.finalize(&mut hashed);
            block[..32].copy_from_slice(&hashed);
        } else {
            block[..key.len()].copy_from_slice(key);
        }

        let mut inner_key = block.map(|byte| byte ^ 0x36);
        let mut inner = Context::new();
        inner.update(&inner_key);
        inner_key.fill(0);

        let context = HmacContext {
            inner,
            outer_key: block.map(|byte| byte ^ 0x5c),
        };
        block.fill(0);
        context
    }

    fn update(ctx: &mut HmacContext, data: &[u8]) {
        ctx.inner.update(data);
    }

    fn finalize(ctx: HmacContext, out: &mut [u8; 32]) {
        let mut inner_hash = [0u8; 32];
        ctx.inner.clone().finalize(&mut inner_hash);

        let mut outer = Context::new();
        outer.update(&ctx.outer_key);
        outer.update(&inner_hash);
        outer.finalize(out);
    }
}

embassy_crypto::hmac_sha256_impl!(HmacDriver);
