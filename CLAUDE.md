# ZeroTag – working notes (formerly pixljszero; the local directory was renamed to `zerotag` on 2026-11-19, matching the git remote)

Amiibo emulator for the M5Stack CardputerZero + Cap CC1101 (ST25R3916 NFC front end). Started from the
CardputerZero application template (MIT). Inspired by pixl.js (GPL-2.0) but **no pixl.js code is copied**:
the project stays MIT, written from the public format, amiitool (MIT) and M5Unit-NFC (MIT). Do not paste
GPL code from pixl.js (reading it for ideas was already done once; prefer public specs).

## State
- Template renamed to pixljszero, then to ZeroTag (ids `zerotag`); framebuffer found via /proc/fb (`fb_st7789v`), not /dev/fb0.
- `src/platform/st25r3916.*`: Cap power-on, SPI, init, NFC-A reader (REQA/WUPA, anticollision, select),
  Type 2 GET_VERSION / READ. Register map in `st25r3916_regs.h` (from M5Unit-NFC, MIT).
- `src/platform/st25r3916_emulation.cpp`: NFC-A NTAG215 emulation (state machine after M5Unit-NFC, Type 2
  commands in software). Test tool `tools/st25r3916_emulate.cpp` (`--blank` or a 540-byte image).
  With an iPhone (NFC Tools): select + READ 02 answered, but iOS then re-reads page 4 three times and
  closes the session; TX completes without errors. Cause open; next test with a Flipper Zero.
- `src/amiibo/`: load key_retail.bin, generate a signed blank NTAG215 from head/tail (random UID),
  verify signatures. `third_party/amiitool` is vendored (MIT).
- `assets/amiibo/amiibo_db.json` (954 entries) + `assets/amiibo/images/<id>.png` (thumbnails, committed on
  purpose: the user decided so). Rebuild with `tools/fetch_amiibo_db.py` / `tools/fetch_amiibo_images.py`.

## Verified on hardware
- IC identity 0x2A (type 0x28, rev 2); NFC-A tag detection with 7-byte UID; GET_VERSION = NTAG215;
  READ pages 0-131; a real amiibo read from a tag verified with the user's keys (format and keys are right).
- NFC-A emulation: a generated amiibo image (Zelda, Smash, from key_retail.bin) is read by amiitool on a
  Flipper Zero and recognised (name, series, UID). Blank tag with NDEF text also read by the Flipper.
  iPhone/NFC Tools selects the tag and reads pages 2 and 4, then re-reads page 4 and closes: iOS-specific,
  cause not proven. `tools/amiibo_gen.cpp` writes an image (host, links mbedtls 3.6.x + amiitool).
- Console acceptance: CLOSED 2026-11-22. The user verified a generated image on a Switch 2 (amiibo customised from it); 3DS/Wii U
  and the save write-back are considered fine by the user, no further test planned. Reference for emulation: M5Unit-NFC `emulation_layer_a_ST25R3916.cpp` (MIT).

## Hardware facts (CardputerZero, Debian 13 trixie aarch64, 352 MB RAM)
- The Cap is unpowered until GPIO26 (POWER_EN) is high AND /sys/class/leds/ext_5v_out/brightness = 1
  (otherwise every register reads 0x00). The driver does both and releases them on close().
- /dev/spidev0.2 only exists after loading the overlay (root): `dtoverlay spi0-spidev2-gpio22-overlay`
  (full name with `-overlay`). SPI mode 1, kernel CS on GPIO22. Lost at reboot.
- IRQ: rising edge, GPIO number unconfirmed (GPIO6 shows consumer "irq"). The driver polls registers.
- No hot-swap of Caps: power off the device before changing the Cap.
- libvlc is irrelevant here (video branch of the other repo was abandoned).
- The device user has passwordless sudo. Ask the user for the device address/SSH access each session.
  Ask before anything that changes the device; clean up /tmp files you create there.

## Keys and data
- `key_retail.bin` (160 bytes) sits at the repo root, git-ignored along with `*.bin`. Never commit it, never
  print it, never copy it elsewhere on my own initiative. Do not search for keys. Exception: the user asked
  (2026-11-19) for one copy on the device at ~/.zerotag/key_retail.bin (mode 600, dir 700), where the app reads it.
- Do not leave tag dumps lying around (shred them after use).

## Tools (build by hand, no CMake target yet)
- Device probe: cross-compile statically so the host libstdc++ does not matter:
  `aarch64-linux-gnu-g++ -std=c++17 -O2 -static -Isrc/platform tools/st25r3916_probe.cpp src/platform/st25r3916.cpp -o st25r_probe`
  then scp and run with sudo: `st25r_probe 30 [dump_file]` (polls 30 s, optional 540-byte dump).
- Host tests need mbedtls (not installed on the host; I built 3.6.2 from source in a scratch dir):
  `tools/amiibo_gen_test.cpp`, `tools/amiibo_verify.cpp` (link amiitool + libmbedcrypto).

## Open items
1. NFC-A emulation: DONE and verified (Flipper, Switch 2). Write-back of what a console writes: considered closed by the user.
2. DONE: mbedtls + `src/amiibo` + amiitool in CMake (`cmake/zerotag-amiibo.cmake`).
3. C++ loader (`amiibo_db`, `amiibo_library`) DONE and tested on the host. LVGL UI: DONE (src/app/zerotag_ui.*, Famicom
   look: 4 px red/cream pixel frame, NES cursor, gold triforce, dark only, Noto font, FR/EN/JA via F2). Designed (artboards at
   https://claude.ai/artifact/CZ2BdxdNsxvi2hyxUNEtrz, 6 screens, 320x170, dark palette), validated by the user.
   Needs more keys in `linux_input.cpp` (only ESC, arrows L/R, Z, C, 4-8 are mapped; the keyboard has all keys).
   Folders decided: backups in ~/amiibo, keys in ~/.zerotag/key_retail.bin, both configurable (the app runs
   as root under APPLaunch, so "~" must resolve to the user's home, not /root).
4. Cross build: DONE without Docker: `cmake --preset cp0-cross-clang` (host clang + the BSP sysroot's GCC 14
   headers, `-DCM0_USE_CLANG=ON` in cmake/toolchains/cp0-aarch64-linux-gnu.cmake), then
   `ninja -C build/cp0-cross-clang` (Release) gives `zerotag` (arm64). Built, linked and RUNNING on the device
   (0.5.0-rev1 installed 2026-11-22; Sym keys not individually checked, dropped on purpose). `.cache` is a symlink to ../CardputerZeroApps/.cache (shared deps, mbedtls is cloned there too).
5. DONE: no boot service any more (store policy). Real reboot tested 2026-11-22: /dev/spidev0.2 is gone after boot, and
   `sudo /usr/lib/zerotag/cap-setup` recreates it (root:spi 660); the app runs as the user (groups spi, gpio). NFC read after the
   reboot: verified too (probe read an NTAG215, identity 0x2A).
6. DONE: store metadata in app-builder.json (author, repo, descriptions, 6 screenshots, network permission true).
7. Optional: `ori.txt` is the user's note with the pixl.js URL; it is untracked.

## Published (2026-11-19)
- Public repo https://github.com/sukoshi1507/zerotag, release v0.3.0 with the .deb (no amiibo images inside).
- History was rewritten before publishing: author sukoshi1507 <17269210+sukoshi1507@users.noreply.github.com>,
  amiibo thumbnails removed (ignored, fetched with tools/fetch_amiibo_images.py; the app also reads ~/.zerotag/images).
- Local git config uses the noreply identity. Private builds with images: -DZEROTAG_BUNDLE_AMIIBO_IMAGES=ON.
- Not done: M5 store submission (app-builder.json still has share_code TODO, categories to confirm).

## Store submission (CardputerZero AppStore) - status 2026-11-19
- Process: `czdev publish --deb X.deb` (CardputerZero/AppBuilder) uploads the .deb to the author's GitHub release
  and opens a PR on CardputerZero/packages; first release needs a short video of the app on a real device in
  the PR comments. Policy script: raw.githubusercontent.com/CardputerZero/packages/main/.github/scripts/store_meta_policy.py
  (passes locally). No systemd system service may run as root: the package ships none; the SPI overlay is loaded
  with `sudo /usr/lib/zerotag/cap-setup` after each boot (like the existing store app amiibo-sim).
- Done: bin_name, categories, share_code ZTAG (free on 2026-11-19), 260 px icon in packaging/icon.png, locales.
- Left for the user: czdev login (GitHub device flow), more 320x170 screenshots (library/detail/emulation; they show
  thumbnails unless ~/.zerotag/images is moved away), the video, and the decision to open the PR.
- Not pushed yet: the 0.3.1 commit (public repo still at v0.3.0, which still has the root service).

## Keyboard and Cap setup (2026-11-19)
- CardputerZero keyboard (tca8418c, kernel driver with 3 layers: base, Sym, Fn; Shift is a normal modifier). The Sym
  symbols arrive as keypad/Japanese key codes wrapped in KEY_CD; official table:
  /usr/share/keymaps/tca8418_keypad_m5stack_keymap.map, implemented in src/platform/linux_input.cpp (verified on the
  device). Docs: https://cardputer.cc/docs/en/CP0_keys.md (keyboard overlay image: .../assets/docs/latest_key_sticker.png).
  APPLaunch keeps /dev/input/event3 open: other readers get no events, so probe keys from inside the app.
- UI screen CapSetup asks for the sudo password (src/platform/cap_setup.cpp: fork + `sudo -S`, no shell, wiped).
  Wrong-password path not tested (this account has passwordless sudo).
- Store PR #504 (v0.3.3) FAILED validation: postinst had `rm -f /etc/...` (CI pattern). Fixed in 0.3.4 (no maintainer script). Next: push + release v0.3.4, close #504, re-run czdev (needs GIT_CONFIG_* env to use HTTPS instead of git@github.com), re-post the video comment.


## Status 2026-11-22 - read this first
- Store: PR #505 (0.3.4) merged 2026-11-20; 0.4.0 published (release v0.4.0, PR CardputerZero/packages#563, merged). Updates now merge
  automatically. Publishing from here: fork sukoshi1507/packages must be in sync with upstream (`gh repo sync`, needs the `workflow`
  token scope: `gh auth refresh -h github.com -s workflow`, run it in the background so its device code stays valid), then
  `GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=url.https://github.com/.insteadOf GIT_CONFIG_VALUE_0=git@github.com: ~/AppBuilder/czdev publish --deb X.deb`.
- 0.5.0 DONE, pushed and PUBLISHED: GitHub release v0.5.0, store PR CardputerZero/packages#672 merged automatically
  2026-11-22 18:05 UTC (CI passed, no manual review needed despite the network permission). Nothing pending on the store side.
- 0.5.0 content: optional online update (config `online`, Settings > Online;
  `src/platform/online_update.*` runs `curl` without a shell, saves ~/.zerotag/amiibo_online.json; `Database::merge` adds unknown
  figures flagged NEW, or SOON when the release date is in the future); search by hex id and accent-insensitive; new store
  screenshots (library, detail, emulation, search; the store allows 6 max). Verified on the device (curl present, download OK,
  setting persisted). The device runs the official 0.5.0-rev1 .deb (installed 2026-11-22 from the release, SHA matches the store CI);
  FR title "Chercher nouveautés" fits in Settings (screenshot checked). After the later reboot (APPLaunch update) the app
  opened the CapSetup screen by itself, the user typed the sudo password and /dev/spidev0.2 was recreated: the CapSetup right-password
  path is verified on the device, so the whole reboot flow works (app only, without running cap-setup by hand).
- DB completeness checked 2026-11-22: bundled 954 = AmiiboAPI 954 (lastUpdated 2026-11-13), no upcoming (future-dated) entries yet.
- Cross build tip: mbedtls-build in the shared .cache can hold stale symlinks that make cmake fail ("File exists"): delete
  ../CardputerZeroApps/.cache/cp0-deps/mbedtls-build and reconfigure. Release needs `cmake --build DIR --config Release`
  then `cpack -G DEB -C Release` (multi-config).
- Device screenshots: APPLaunch owns the keyboard, so the user navigates and I capture over ssh
  (`sudo ffmpeg -f fbdev -i /dev/fb0 -frames:v 1`). Device address: ask the user each session.

## Status 2026-11-19 (end of day) - older
- Everything is committed and pushed (origin/main = 7056a01). Public release v0.3.4 is the only one; the code in main is ahead of it.
- Store PR CardputerZero/packages#505 (v0.3.4): CI passed, video comments posted, OPEN with no maintainer reply as of 18:47 UTC.
  First release is merged by hand. Do NOT comment again or publish anything new until the maintainer answers.
  When it is merged: bump to 0.4.0 (CMakeLists.txt, app-builder.json, README), build the .deb, GitHub release v0.4.0,
  then `czdev publish --deb` (needs the HTTPS GIT_CONFIG_* trick, see above) - later updates merge automatically.
- Added after 0.3.4 (in main, installed on the device as 0.3.4-rev1 build from main):
  * Library: F2 favourite, F3 tabs All/Favourites/Recent (state in ~/.zerotag/library_state.txt), F4 scan a real amiibo
    (src/app/zerotag_ui.cpp scan_worker; the chip needs init_nfc_a() after open(), forgetting it left the chip as a dead reader).
  * Catalogue: F2 facets (amiibo series, game series, type; the DB has no platform field).
  * Sound (src/platform/audio.*): synthesised effects + chiptune played through `aplay` on a pipe; Settings > Sound (config key `sound`).
    TU-DU-TUUU after a successful scan.
  * Settings > Manual: 5-page demoscene screen (copper bars, sine scroller, starfield, music), FR/EN/JA.
  * Perf: main loop sleeps for real (lv_delay_ms busy-waited, 100% CPU); LV_USE_ASSERT_OBJ is off (it made each invalidation walk the tree).
    Idle ~0.3% CPU, manual screen ~34%.
- Untested (older list, see the 2026-11-22 status): wrong sudo password path (not observable on this account).
- Debug tips that worked: framebuffer screenshot over ssh (`ffmpeg -f fbdev -i /dev/fb0 -frames:v 1`), `strace -c -p`, `gdb -batch -p PID -ex "bt 8"`
  sampling, and a standalone probe (tools/st25r3916_probe.cpp) to separate hardware from app problems. Keep tag dumps off the disk.
- Build: scratch dir under the session scratchpad (cp0clang) keeps absolute paths to the old directory name: reconfigure it
  (`cmake --preset cp0-cross-clang -B <dir> -DFETCHCONTENT_SOURCE_DIR_MBEDTLS=../CardputerZeroApps/.cache/cp0-deps/mbedtls-src`) or use a fresh dir.

## Ideas not done yet (2026-11-22)
- iOS re-reads page 4 then closes the session (cause unproven); use the IRQ line instead of register polling.
- CTest targets for tools/*_test.cpp (host tests need only src/amiibo + a parse_id stub, see amiibo_db_test), CI for the cross build.
- Show the release date of an amiibo in the catalogue panel; check upcoming amiibo when AmiiboAPI lists future-dated ones (none on 2026-11-22).
- Wrong sudo password: cannot be observed on this account (sudoers NOPASSWD: ALL, sudo accepts any password; the code path
  PasswordRefused exists in cap_setup.cpp but needs an account without NOPASSWD). JA layout of the new screens checked by the user
  2026-11-22: OK.

## Rust port (branch `rust-port`, started 2026-11-24, local only, nothing pushed)
- Decided with the user: a Rust fork of the project in the same repo (workspace `rust/`), UI later with `embedded-graphics`
  on the framebuffer (`fb_st7789v`, 320x170) instead of LVGL. The C++ app stays the published version until the port has
  the same features. No GitHub repo created for it yet (ask first).
- Toolchain: no rustup/cargo on this host; use `mise x rust@stable -- cargo ...` (user space, rust 1.100). Run from `rust/`.
  aarch64 target not installed yet (`rustup target add` is not available through mise's plain install; check before cross build).
- DONE in `rust/zerotag-core`: `amiibo::{Keys, generate, rotate_uid, verify, parse_id}` (RustCrypto: aes, ctr, hmac, sha2; replaces
  mbedtls + vendored amiitool) and `amiibo::{Database, Entry, Facet}` (serde_json; load, merge online, accent-insensitive search, facets).
  11 unit tests (`cargo test`, synthetic keys, plus the bundled 954-entry catalogue).
- Crypto verified against the C++ build with the real key_retail.bin (read in place, never copied/printed): C++ gen -> Rust verify,
  Rust gen -> C++ verify, Rust rotate_uid -> C++ verify, all valid. The C++ tools were built in the scratchpad against
  /usr/lib/libmbedcrypto.so.3.6.7 and the headers in ../CardputerZeroApps/.cache/cp0-deps/mbedtls-src/include.
  Rust examples: `cargo build --release --examples` gives amiibo_gen / amiibo_verify / amiibo_rotate (same CLI as tools/).
- Next: library_state + config, platform (spidev/gpio ST25R3916 driver, emulation state machine, keyboard via evdev, audio,
  online_update, cap_setup), then the embedded-graphics UI. The NFC parts can only be checked on the device (ask for its address).
