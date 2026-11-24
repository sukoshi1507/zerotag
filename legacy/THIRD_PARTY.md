# Third-party components (previous C++ version, `legacy/`)

ZeroTag itself is MIT licensed (see `LICENSE`). It uses or derives from the following.

| Component | Use | License |
| --- | --- | --- |
| [amiitool](https://github.com/socram8888/amiitool) (vendored in `legacy/third_party/amiitool`) | amiibo key derivation, encryption and signatures | MIT (see `legacy/third_party/amiitool/LICENSE`) |
| [Mbed TLS](https://github.com/Mbed-TLS/mbedtls) 3.6.x | AES and HMAC for amiitool, fetched at build time and linked statically | Apache-2.0 (or GPL-2.0-or-later); used under Apache-2.0 |
| [LVGL](https://github.com/lvgl/lvgl) 9.5 | user interface | MIT |
| [M5Unit-NFC](https://github.com/m5stack/M5Unit-NFC) | ST25R3916 register map and the NFC-A passive-target (card emulation) sequence, re-implemented for Linux userspace in `legacy/src/platform/st25r3916_*` | MIT, (c) 2025 M5Stack Technology CO LTD |
| CardputerZero application template | project skeleton (build system, packaging, platform layer) | MIT, (c) M5Stack CardputerZero Community |
| [AmiiboAPI](https://www.amiiboapi.org) | character list in `assets/amiibo/amiibo_db.json` (metadata only) | see the AmiiboAPI project |
| Kenney Input Prompts, Phosphor Icons | fonts in `assets/fonts` | CC0 and MIT, see `assets/fonts/LICENSE.txt` |
| Noto Sans CJK (system font on the device) | Latin and Japanese text | SIL OFL 1.1 |

## Not included

- **Amiibo keys** (`key_retail.bin`): you must supply your own, ZeroTag never ships or downloads them.
- **Amiibo artwork**: the thumbnails come from the AmiiboAPI image repository and belong to Nintendo. They are
  included in the `.deb` packages for display only and are **not** part of this source repository;
  `tools/fetch_amiibo_images.py` downloads them on your machine.
- pixl.js (GPL-2.0) is an inspiration only: no code from it is used.

Amiibo is a trademark of Nintendo. ZeroTag is not affiliated with or endorsed by Nintendo, M5Stack or AmiiboAPI.
