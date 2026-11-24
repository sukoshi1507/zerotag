// SPDX-License-Identifier: MIT
//! Amiibo NTAG215 images: key derivation, encryption and signatures.
//! Port of src/amiibo/amiibo_image.cpp and of amiitool (MIT, Marcos Del Sol Vives / javiMaD).

mod crypto;
mod db;
mod image;
mod library;
mod state;

pub use crypto::{Keys, KEYS_FILE_SIZE};
pub use db::{Database, Entry, Facet, FacetCount};
pub use library::{read_image, write_image, Item, Library};
pub use state::{LibraryState, RECENT_MAX};
pub use image::{generate, parse_id, rotate_uid, verify, Error, TagImage, NTAG215_SIZE};
