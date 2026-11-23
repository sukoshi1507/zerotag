// SPDX-License-Identifier: MIT
//! Port of amiitool's keygen.c, drbg.c and amiibo.c.

use aes::cipher::{KeyIvInit, StreamCipher};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::io;
use std::path::Path;
use zeroize::Zeroize;

type HmacSha256 = Hmac<Sha256>;
type Aes128Ctr = ctr::Ctr128BE<aes::Aes128>;

/// Size of the internal (decrypted) layout.
pub(crate) const PLAIN_SIZE: usize = 520;
pub const KEYS_FILE_SIZE: usize = 160;

const HMAC_POS_DATA: usize = 0x008;
const HMAC_POS_TAG: usize = 0x1B4;

/// One set of master keys (80 bytes in key_retail.bin).
#[derive(Clone)]
struct MasterKeys {
    hmac_key: [u8; 16],
    type_string: [u8; 14],
    magic_bytes_size: u8,
    magic_bytes: [u8; 16],
    xor_pad: [u8; 32],
}

impl MasterKeys {
    fn parse(b: &[u8]) -> Option<Self> {
        // layout: hmacKey[16] typeString[14] rfu(1) magicBytesSize(1) magicBytes[16] xorPad[32]
        let magic_bytes_size = b[31];
        if magic_bytes_size > 16 {
            return None;
        }
        Some(Self {
            hmac_key: b[0..16].try_into().ok()?,
            type_string: b[16..30].try_into().ok()?,
            magic_bytes_size,
            magic_bytes: b[32..48].try_into().ok()?,
            xor_pad: b[48..80].try_into().ok()?,
        })
    }

    fn wipe(&mut self) {
        self.hmac_key.zeroize();
        self.type_string.zeroize();
        self.magic_bytes.zeroize();
        self.xor_pad.zeroize();
        self.magic_bytes_size = 0;
    }
}

/// Master keys (key_retail.bin, 160 bytes) supplied by the user. Never embedded in the project.
pub struct Keys {
    data: MasterKeys,
    tag: MasterKeys,
}

impl Drop for Keys {
    fn drop(&mut self) {
        self.data.wipe();
        self.tag.wipe();
    }
}

impl Keys {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != KEYS_FILE_SIZE {
            return None;
        }
        Some(Self { data: MasterKeys::parse(&bytes[0..80])?, tag: MasterKeys::parse(&bytes[80..160])? })
    }

    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        let mut buf = std::fs::read(path)?;
        let keys = Self::from_bytes(&buf[..buf.len().min(KEYS_FILE_SIZE)]);
        buf.zeroize();
        keys.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not a valid key file"))
    }
}

struct Derived {
    aes_key: [u8; 16],
    aes_iv: [u8; 16],
    hmac_key: [u8; 16],
}

impl Drop for Derived {
    fn drop(&mut self) {
        self.aes_key.zeroize();
        self.aes_iv.zeroize();
        self.hmac_key.zeroize();
    }
}

fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
    for p in parts {
        mac.update(p);
    }
    mac.finalize().into_bytes().into()
}

/// keygen.c: nfc3d_keygen_prepare_seed.
fn prepare_seed(keys: &MasterKeys, seed: &[u8; 64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(480);
    // memccpy(..., '\0', 14): copies up to and including the first NUL (or all 14 bytes).
    match keys.type_string.iter().position(|&c| c == 0) {
        Some(i) => out.extend_from_slice(&keys.type_string[..=i]),
        None => out.extend_from_slice(&keys.type_string),
    }
    let size = keys.magic_bytes_size as usize;
    out.extend_from_slice(&seed[..16 - size]);
    out.extend_from_slice(&keys.magic_bytes[..size]);
    out.extend_from_slice(&seed[0x10..0x20]);
    out.extend((0..32).map(|i| seed[32 + i] ^ keys.xor_pad[i]));
    out
}

/// drbg.c: HMAC-SHA256 in counter mode over (counter_be16 || seed), 48 bytes of output.
fn derive(keys: &MasterKeys, seed: &[u8; 64]) -> Derived {
    let mut prepared = prepare_seed(keys, seed);
    let mut stream = [0u8; 64];
    for (i, chunk) in stream.chunks_mut(32).enumerate() {
        let counter = (i as u16).to_be_bytes();
        chunk.copy_from_slice(&hmac(&keys.hmac_key, &[&counter, &prepared]));
    }
    prepared.zeroize();
    let d = Derived {
        aes_key: stream[0..16].try_into().unwrap(),
        aes_iv: stream[16..32].try_into().unwrap(),
        hmac_key: stream[32..48].try_into().unwrap(),
    };
    stream.zeroize();
    d
}

fn calc_seed(dump: &[u8; PLAIN_SIZE]) -> [u8; 64] {
    let mut key = [0u8; 64];
    key[0x00..0x02].copy_from_slice(&dump[0x029..0x02B]);
    key[0x10..0x18].copy_from_slice(&dump[0x1D4..0x1DC]);
    key[0x18..0x20].copy_from_slice(&dump[0x1D4..0x1DC]);
    key[0x20..0x40].copy_from_slice(&dump[0x1E8..0x208]);
    key
}

fn keygen(master: &MasterKeys, dump: &[u8; PLAIN_SIZE]) -> Derived {
    let mut seed = calc_seed(dump);
    let d = derive(master, &seed);
    seed.zeroize();
    d
}

fn cipher(keys: &Derived, input: &[u8; PLAIN_SIZE], out: &mut [u8; PLAIN_SIZE]) {
    let mut c = Aes128Ctr::new((&keys.aes_key).into(), (&keys.aes_iv).into());
    out[0x02C..0x1B4].copy_from_slice(&input[0x02C..0x1B4]);
    c.apply_keystream(&mut out[0x02C..0x1B4]);
    out[0x000..0x008].copy_from_slice(&input[0x000..0x008]);
    out[0x028..0x02C].copy_from_slice(&input[0x028..0x02C]);
    out[0x1D4..0x208].copy_from_slice(&input[0x1D4..0x208]);
}

fn tag_to_internal(tag: &[u8], i: &mut [u8; PLAIN_SIZE]) {
    i[0x000..0x008].copy_from_slice(&tag[0x008..0x010]);
    i[0x008..0x028].copy_from_slice(&tag[0x080..0x0A0]);
    i[0x028..0x04C].copy_from_slice(&tag[0x010..0x034]);
    i[0x04C..0x1B4].copy_from_slice(&tag[0x0A0..0x208]);
    i[0x1B4..0x1D4].copy_from_slice(&tag[0x034..0x054]);
    i[0x1D4..0x1DC].copy_from_slice(&tag[0x000..0x008]);
    i[0x1DC..0x208].copy_from_slice(&tag[0x054..0x080]);
}

fn internal_to_tag(i: &[u8; PLAIN_SIZE], tag: &mut [u8]) {
    tag[0x008..0x010].copy_from_slice(&i[0x000..0x008]);
    tag[0x080..0x0A0].copy_from_slice(&i[0x008..0x028]);
    tag[0x010..0x034].copy_from_slice(&i[0x028..0x04C]);
    tag[0x0A0..0x208].copy_from_slice(&i[0x04C..0x1B4]);
    tag[0x034..0x054].copy_from_slice(&i[0x1B4..0x1D4]);
    tag[0x000..0x008].copy_from_slice(&i[0x1D4..0x1DC]);
    tag[0x054..0x080].copy_from_slice(&i[0x1DC..0x208]);
}

/// Decrypts a tag image (at least 0x208 bytes). `None` if the signatures do not verify.
pub(crate) fn unpack(keys: &Keys, tag: &[u8]) -> Option<[u8; PLAIN_SIZE]> {
    let mut internal = [0u8; PLAIN_SIZE];
    tag_to_internal(tag, &mut internal);

    let data_keys = keygen(&keys.data, &internal);
    let tag_keys = keygen(&keys.tag, &internal);

    let mut plain = [0u8; PLAIN_SIZE];
    cipher(&data_keys, &internal, &mut plain);

    // The tag HMAC is regenerated first: the data HMAC depends on it.
    let tag_hmac = hmac(&tag_keys.hmac_key, &[&plain[0x1D4..0x208]]);
    plain[HMAC_POS_TAG..HMAC_POS_TAG + 32].copy_from_slice(&tag_hmac);
    let data_hmac = hmac(&data_keys.hmac_key, &[&plain[0x029..0x208]]);
    plain[HMAC_POS_DATA..HMAC_POS_DATA + 32].copy_from_slice(&data_hmac);

    let ok = plain[HMAC_POS_DATA..HMAC_POS_DATA + 32] == internal[HMAC_POS_DATA..HMAC_POS_DATA + 32]
        && plain[HMAC_POS_TAG..HMAC_POS_TAG + 32] == internal[HMAC_POS_TAG..HMAC_POS_TAG + 32];
    internal.zeroize();
    if ok {
        Some(plain)
    } else {
        plain.zeroize();
        None
    }
}

/// Signs and encrypts a decrypted image into `tag` (at least 0x208 bytes, expected zeroed).
pub(crate) fn pack(keys: &Keys, plain: &[u8; PLAIN_SIZE], tag: &mut [u8]) {
    let tag_keys = keygen(&keys.tag, plain);
    let data_keys = keygen(&keys.data, plain);

    let mut buf = [0u8; PLAIN_SIZE];
    let tag_hmac = hmac(&tag_keys.hmac_key, &[&plain[0x1D4..0x208]]);
    buf[HMAC_POS_TAG..HMAC_POS_TAG + 32].copy_from_slice(&tag_hmac);
    let data_hmac = hmac(&data_keys.hmac_key, &[&plain[0x029..0x1B4], &tag_hmac, &plain[0x1D4..0x208]]);
    buf[HMAC_POS_DATA..HMAC_POS_DATA + 32].copy_from_slice(&data_hmac);

    let mut out = buf;
    cipher(&data_keys, plain, &mut out);
    // `cipher` leaves the HMAC regions untouched (they come from `buf`).
    internal_to_tag(&out, tag);
    buf.zeroize();
    out.zeroize();
}
