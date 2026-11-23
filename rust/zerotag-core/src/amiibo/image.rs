// SPDX-License-Identifier: MIT
//! Blank amiibo image generation, UID rotation and verification (port of amiibo_image.cpp).

use super::crypto::{pack, unpack, Keys, PLAIN_SIZE};
use std::fmt;
use zeroize::Zeroize;

pub const NTAG215_SIZE: usize = 540; // 135 pages of 4 bytes
pub type TagImage = [u8; NTAG215_SIZE];

// Offsets in the internal (decrypted) layout.
const PLAIN_STATIC_LOCK: usize = 0x000; // BCC1, internal 0x48, lock bytes 0F E0, capability container
const PLAIN_MAGIC: usize = 0x028; // 0xA5, then the write counter
const PLAIN_UID: usize = 0x1D4; // UID0-2, BCC0, UID3-6
const PLAIN_MODEL: usize = 0x1DC; // head (4 bytes) + tail (4 bytes)
const PLAIN_SALT: usize = 0x1E8; // 32 random bytes feeding the key derivation

// Pages 130..134 of the NTAG215 (not covered by the signatures).
const TAIL_DYNAMIC_LOCK: usize = 0x208;
const TAIL_PASSWORD: usize = 0x214;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// The image does not verify with these keys.
    BadImage,
    /// The system random source failed.
    Random,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadImage => f.write_str("the image does not verify with these keys"),
            Error::Random => f.write_str("random source unavailable"),
        }
    }
}

impl std::error::Error for Error {}

/// Parses the 16 hex characters of an AmiiboAPI `id` (head + tail, e.g. "01010000000e0002").
pub fn parse_id(id: &str) -> Option<(u32, u32)> {
    if id.len() != 16 || !id.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u64::from_str_radix(id, 16).ok()?;
    Some(((v >> 32) as u32, v as u32))
}

/// NTAG215 access password derived from the 7-byte UID.
fn derive_password(uid: &[u8; 7]) -> [u8; 4] {
    [0xAA ^ uid[1] ^ uid[3], 0x55 ^ uid[2] ^ uid[4], 0xAA ^ uid[3] ^ uid[5], 0x55 ^ uid[4] ^ uid[6]]
}

fn random_uid() -> Result<[u8; 7], Error> {
    let mut uid = [0u8; 7];
    getrandom::getrandom(&mut uid[1..]).map_err(|_| Error::Random)?;
    uid[0] = 0x04; // NXP manufacturer byte
    Ok(uid)
}

/// Writes UID0-2, BCC0, UID3-6 and BCC1 into the decrypted image.
fn set_uid(plain: &mut [u8; PLAIN_SIZE], uid: &[u8; 7]) {
    let bcc0 = 0x88 ^ uid[0] ^ uid[1] ^ uid[2];
    let bcc1 = uid[3] ^ uid[4] ^ uid[5] ^ uid[6];
    plain[PLAIN_UID..PLAIN_UID + 3].copy_from_slice(&uid[0..3]);
    plain[PLAIN_UID + 3] = bcc0;
    plain[PLAIN_UID + 4..PLAIN_UID + 8].copy_from_slice(&uid[3..7]);
    plain[PLAIN_STATIC_LOCK] = bcc1;
}

/// Builds a blank, signed NTAG215 image for the character `head`/`tail`, with a random UID.
pub fn generate(keys: &Keys, head: u32, tail: u32) -> Result<TagImage, Error> {
    let uid = random_uid()?;
    let mut plain = [0u8; PLAIN_SIZE];
    plain[PLAIN_STATIC_LOCK + 1..PLAIN_STATIC_LOCK + 8].copy_from_slice(&[0x48, 0x0F, 0xE0, 0xF1, 0x10, 0xFF, 0xEE]);
    plain[PLAIN_MAGIC] = 0xA5;
    set_uid(&mut plain, &uid);
    plain[PLAIN_MODEL..PLAIN_MODEL + 4].copy_from_slice(&head.to_be_bytes());
    plain[PLAIN_MODEL + 4..PLAIN_MODEL + 8].copy_from_slice(&tail.to_be_bytes());
    getrandom::getrandom(&mut plain[PLAIN_SALT..PLAIN_SALT + 32]).map_err(|_| Error::Random)?;

    let mut out = [0u8; NTAG215_SIZE];
    pack(keys, &plain, &mut out);

    const DYNAMIC_LOCK: [u8; 12] = [0x01, 0x00, 0x0F, 0xBD, 0x00, 0x00, 0x00, 0x04, 0x5F, 0x00, 0x00, 0x00];
    out[TAIL_DYNAMIC_LOCK..TAIL_DYNAMIC_LOCK + 12].copy_from_slice(&DYNAMIC_LOCK);
    out[TAIL_PASSWORD..TAIL_PASSWORD + 4].copy_from_slice(&derive_password(&uid));
    out[TAIL_PASSWORD + 4] = 0x80; // PACK
    out[TAIL_PASSWORD + 5] = 0x80;
    plain.zeroize();
    Ok(out)
}

/// Gives an existing image a new random UID while keeping all its data (owner, game saves...).
/// The UID is part of the key derivation and of the signatures, so the image is decrypted with the
/// old UID and encrypted and signed again with the new one; the NTAG password is updated too.
/// The image is left untouched if it does not verify.
pub fn rotate_uid(keys: &Keys, image: &mut TagImage) -> Result<(), Error> {
    let mut plain = unpack(keys, image).ok_or(Error::BadImage)?;
    let uid = random_uid()?;
    set_uid(&mut plain, &uid);

    // Pages 130..134 are not covered by the signatures: keep them, with the password of the new UID.
    let mut tail = [0u8; NTAG215_SIZE - TAIL_DYNAMIC_LOCK];
    tail.copy_from_slice(&image[TAIL_DYNAMIC_LOCK..]);

    let mut out = [0u8; NTAG215_SIZE];
    pack(keys, &plain, &mut out);
    out[TAIL_DYNAMIC_LOCK..].copy_from_slice(&tail);
    out[TAIL_PASSWORD..TAIL_PASSWORD + 4].copy_from_slice(&derive_password(&uid));

    *image = out;
    plain.zeroize();
    Ok(())
}

/// Checks the signatures of an image and extracts its head/tail. `None` if it does not verify.
pub fn verify(keys: &Keys, image: &TagImage) -> Option<(u32, u32)> {
    let mut plain = unpack(keys, image)?;
    let head = u32::from_be_bytes(plain[PLAIN_MODEL..PLAIN_MODEL + 4].try_into().unwrap());
    let tail = u32::from_be_bytes(plain[PLAIN_MODEL + 4..PLAIN_MODEL + 8].try_into().unwrap());
    plain.zeroize();
    Some((head, tail))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic keys (NOT the retail ones): enough to exercise the whole pipeline.
    fn fake_keys() -> Keys {
        let mut b = [0u8; 160];
        for (i, x) in b.iter_mut().enumerate() {
            *x = (i as u8).wrapping_mul(37).wrapping_add(11);
        }
        for set in [0usize, 80] {
            b[set + 16..set + 30].copy_from_slice(b"unfixed infos\0");
            b[set + 30] = 0;
            b[set + 31] = 14; // magic bytes size
        }
        Keys::from_bytes(&b).unwrap()
    }

    #[test]
    fn parse_id_ok() {
        assert_eq!(parse_id("01010000000e0002"), Some((0x01010000, 0x000e0002)));
        assert_eq!(parse_id("01010000000E0002"), Some((0x01010000, 0x000e0002)));
        assert_eq!(parse_id("0101"), None);
        assert_eq!(parse_id("0101000000zz0002"), None);
    }

    #[test]
    fn generate_verifies() {
        let k = fake_keys();
        let img = generate(&k, 0x01010000, 0x000e0002).unwrap();
        assert_eq!(verify(&k, &img), Some((0x01010000, 0x000e0002)));
        assert_eq!(&img[0x208..0x214], &[0x01, 0x00, 0x0F, 0xBD, 0, 0, 0, 0x04, 0x5F, 0, 0, 0]);
        assert_eq!(&img[0x218..0x21A], &[0x80, 0x80]);
    }

    #[test]
    fn tampered_image_is_rejected() {
        let k = fake_keys();
        let mut img = generate(&k, 1, 2).unwrap();
        img[0x100] ^= 1;
        assert_eq!(verify(&k, &img), None);
    }

    #[test]
    fn rotate_changes_uid_keeps_model() {
        let k = fake_keys();
        let mut img = generate(&k, 0x01010000, 0x000e0002).unwrap();
        let old = img;
        rotate_uid(&k, &mut img).unwrap();
        assert_ne!(img[..8], old[..8]);
        assert_eq!(verify(&k, &img), Some((0x01010000, 0x000e0002)));
    }

    #[test]
    fn rotate_rejects_bad_image_and_leaves_it() {
        let k = fake_keys();
        let mut img = [0x55u8; NTAG215_SIZE];
        assert_eq!(rotate_uid(&k, &mut img), Err(Error::BadImage));
        assert_eq!(img, [0x55u8; NTAG215_SIZE]);
    }
}
