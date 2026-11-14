//! P-256 ECDH and ECDSA signing with the point multiplication on the ECC accelerator.
//!
//! The ESP32-C6's accelerator multiplies and checks points, but does not add them, and has no
//! arithmetic modulo the group order. The scalar arithmetic of a signature, and the verification,
//! run in software.

use embassy_crypto::{
    Error,
    driver::{P256Ecdh, P256Ecdsa, P256Point, P256Scalar, P256Signature},
    rng_fill_bytes,
};
use esp_hal::ecc::EllipticCurve;
use p256::{
    AffinePoint, FieldBytes, NistP256, Scalar,
    ecdsa::{Signature, VerifyingKey, signature::hazmat::PrehashVerifier},
    elliptic_curve::{
        Field, PrimeField,
        scalar::IsHigh,
        sec1::{FromSec1Point, Sec1Point},
    },
};

/// The generator, big-endian.
const GX: [u8; 32] = [
    0x6b, 0x17, 0xd1, 0xf2, 0xe1, 0x2c, 0x42, 0x47, 0xf8, 0xbc, 0xe6, 0xe5, 0x63, 0xa4, 0x40, 0xf2,
    0x77, 0x03, 0x7d, 0x81, 0x2d, 0xeb, 0x33, 0xa0, 0xf4, 0xa1, 0x39, 0x45, 0xd8, 0x98, 0xc2, 0x96,
];
const GY: [u8; 32] = [
    0x4f, 0xe3, 0x42, 0xe2, 0xfe, 0x1a, 0x7f, 0x9b, 0x8e, 0xe7, 0xeb, 0x4a, 0x7c, 0x0f, 0x9e, 0x16,
    0x2b, 0xce, 0x33, 0x57, 0x6b, 0x31, 0x5e, 0xce, 0xcb, 0xb6, 0x40, 0x68, 0x37, 0xbf, 0x51, 0xf5,
];
/// The group order `n`, big-endian.
const N: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17, 0x9e, 0x84, 0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63, 0x25, 0x51,
];

/// `None` unless `0 < k < n`.
fn nonzero_scalar(k: &[u8; 32]) -> Option<Scalar> {
    let s: Option<Scalar> = Scalar::from_repr(FieldBytes::from(*k)).into();
    s.filter(|s| !bool::from(s.is_zero()))
}

/// `x mod n` for any 256-bit `x`, which is below `2n`. For public values only: the time depends
/// on `x`.
fn reduce(x: &[u8; 32]) -> Scalar {
    if let Some(s) = Option::<Scalar>::from(Scalar::from_repr(FieldBytes::from(*x))) {
        return s;
    }
    let mut difference = [0u8; 32];
    let mut borrow = 0u16;
    for i in (0..32).rev() {
        let value = u16::from(x[i])
            .wrapping_sub(u16::from(N[i]))
            .wrapping_sub(borrow);
        difference[i] = value as u8;
        borrow = (value >> 8) & 1;
    }
    Option::<Scalar>::from(Scalar::from_repr(FieldBytes::from(difference)))
        .expect("x - n is below n")
}

/// `k * P` on the accelerator. With `check`, the accelerator first checks that `P` is on the
/// curve. `k` is a valid nonzero scalar, so the result is never the identity.
fn multiply(k: &[u8; 32], x: &[u8; 32], y: &[u8; 32], check: bool) -> Result<P256Point, Error> {
    // The accelerator's memories take little-endian numbers.
    let le = |be: &[u8; 32]| {
        let mut le = *be;
        le.reverse();
        le
    };
    let (k, x, y) = (le(k), le(x), le(y));
    let (mut rx, mut ry) = ([0u8; 32], [0u8; 32]);

    if check {
        let mut op = EllipticCurve::P256
            .affine_point_verification_multiplication(&k, &x, &y)
            .and_then(|op| op.with_affine_point_result(&mut rx, &mut ry))
            .map_err(|_| Error::HardwareError)?;
        op.process().wait_blocking();
        if !op.point_on_curve() {
            return Err(Error::InvalidKey);
        }
    } else {
        EllipticCurve::P256
            .affine_point_multiplication(&k, &x, &y)
            .and_then(|op| op.with_affine_point_result(&mut rx, &mut ry))
            .map_err(|_| Error::HardwareError)?
            .process()
            .wait_blocking();
    }

    rx.reverse();
    ry.reverse();
    Ok(P256Point { x: rx, y: ry })
}

fn public_key(k: &P256Scalar) -> Result<P256Point, Error> {
    nonzero_scalar(&k.0).ok_or(Error::InvalidKey)?;
    multiply(&k.0, &GX, &GY, false)
}

struct EcdhDriver;

impl P256Ecdh for EcdhDriver {
    fn public_key(k: &P256Scalar) -> Result<P256Point, Error> {
        public_key(k)
    }

    fn shared_secret(k: &P256Scalar, peer: &P256Point) -> Result<[u8; 32], Error> {
        nonzero_scalar(&k.0).ok_or(Error::InvalidKey)?;
        // A peer point off the curve would leak the key: the accelerator checks it.
        multiply(&k.0, &peer.x, &peer.y, true).map(|point| point.x)
    }
}

embassy_crypto::p256_ecdh_impl!(EcdhDriver);

struct EcdsaDriver;

impl P256Ecdsa for EcdsaDriver {
    fn public_key(k: &P256Scalar) -> Result<P256Point, Error> {
        public_key(k)
    }

    /// ECDSA with a random nonce, `s` normalized low.
    fn sign(k: &P256Scalar, digest: &[u8; 32]) -> Result<P256Signature, Error> {
        let d = nonzero_scalar(&k.0).ok_or(Error::InvalidKey)?;
        let z = reduce(digest);
        loop {
            let mut nonce_bytes = [0u8; 32];
            rng_fill_bytes(&mut nonce_bytes);
            let Some(nonce) = nonzero_scalar(&nonce_bytes) else {
                continue;
            };
            let point = multiply(&nonce_bytes, &GX, &GY, false)?;
            nonce_bytes.fill(0);

            let r = reduce(&point.x);
            if bool::from(r.is_zero()) {
                continue;
            }
            let inverse: Option<Scalar> = nonce.invert().into();
            let mut s = inverse.expect("the nonce is nonzero") * (z + r * d);
            if bool::from(s.is_zero()) {
                continue;
            }
            if bool::from(s.is_high()) {
                s = -s;
            }
            return Ok(P256Signature {
                r: P256Scalar(r.to_repr().into()),
                s: P256Scalar(s.to_repr().into()),
            });
        }
    }

    fn verify(q: &P256Point, digest: &[u8; 32], sig: &P256Signature) -> Result<(), Error> {
        let encoded = Sec1Point::<NistP256>::from_affine_coordinates(
            &FieldBytes::from(q.x),
            &FieldBytes::from(q.y),
            false,
        );
        let q: Option<AffinePoint> = AffinePoint::from_sec1_point(&encoded).into();
        let key = VerifyingKey::from_affine(q.ok_or(Error::InvalidKey)?)
            .map_err(|_| Error::InvalidKey)?;
        let signature =
            Signature::from_scalars(FieldBytes::from(sig.r.0), FieldBytes::from(sig.s.0))
                .map_err(|_| Error::InvalidSignature)?;
        key.verify_prehash(digest, &signature)
            .map_err(|_| Error::InvalidSignature)
    }
}

embassy_crypto::p256_ecdsa_impl!(EcdsaDriver);
