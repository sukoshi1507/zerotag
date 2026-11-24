# Third-party components

ZeroTag itself is MIT licensed (see `LICENSE`). The Rust version uses or derives from the following.

| Component | Use | License |
| --- | --- | --- |
| [amiitool](https://github.com/socram8888/amiitool) | reference for the amiibo key derivation, encryption and signatures, re-implemented in Rust (a vendored C copy lives in `legacy/third_party/amiitool`) | MIT |
| [M5Unit-NFC](https://github.com/m5stack/M5Unit-NFC) | ST25R3916 register map and the NFC-A passive-target (card emulation) sequence, re-implemented for Linux userspace in `rust/zerotag-core/src/nfc/` | MIT, (c) 2025 M5Stack Technology CO LTD |
| CardputerZero application template | project skeleton (packaging, launcher integration) | MIT, (c) M5Stack CardputerZero Community |
| [AmiiboAPI](https://www.amiiboapi.org) | character list in `assets/amiibo/amiibo_db.json` and the amiibo thumbnails (display only) | see the AmiiboAPI project |
| [Noto Sans CJK](https://github.com/notofonts/noto-cjk) | the interface font, read from the device (`fonts-noto-cjk`), not shipped | SIL Open Font License 1.1 |
| [RustCrypto](https://github.com/RustCrypto) `aes`, `ctr`, `hmac`, `sha2` | AES-128-CTR and HMAC-SHA256 | MIT OR Apache-2.0 |
| [ab_glyph](https://github.com/alexheretic/ab-glyph) | glyph outlines and rasterisation | Apache-2.0 |
| [memmap2](https://github.com/RazrFalcon/memmap2-rs) | memory-maps the font files | MIT OR Apache-2.0 |
| [png](https://github.com/image-rs/image-png) | thumbnails and screenshots | MIT OR Apache-2.0 |
| [serde_json](https://github.com/serde-rs/json) | catalogue files | MIT OR Apache-2.0 |
| [spidev](https://github.com/rust-embedded/rust-spidev), [gpiocdev](https://github.com/warthog618/gpiocdev-rs) | SPI and GPIO access to the Cap | MIT/Apache-2.0, Apache-2.0 OR MIT |
| [getrandom](https://github.com/rust-random/getrandom), [zeroize](https://github.com/RustCrypto/utils), [os_pipe](https://github.com/oconnor663/os_pipe.rs), [chrono](https://github.com/chronotope/chrono) | random UIDs, wiping secrets, helper processes, file names | MIT OR Apache-2.0 (os_pipe: MIT) |

The dependency tree (`cargo metadata`) only contains permissive licenses (MIT, Apache-2.0, BSD-3-Clause, Zlib, Unicode-3.0
and similar). pixl.js was only an inspiration for the idea: no code of it is used. Amiibo is a trademark of Nintendo.

The components of the previous C++ version (LVGL, Mbed TLS, ...) are listed in `legacy/THIRD_PARTY.md`.
