// SPDX-License-Identifier: MIT
//! Amiibo NTAG215 images: key derivation, encryption and signatures.
//! Port of src/amiibo/amiibo_image.cpp and of amiitool (MIT, Marcos Del Sol Vives / javiMaD).

mod crypto;
mod image;

pub use crypto::{Keys, KEYS_FILE_SIZE};
pub use image::{generate, parse_id, rotate_uid, verify, Error, TagImage, NTAG215_SIZE};
