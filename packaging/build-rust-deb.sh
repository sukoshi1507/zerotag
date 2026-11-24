#!/bin/sh
# SPDX-License-Identifier: MIT
# Builds the Debian package of the Rust version of ZeroTag (arm64, statically linked: it needs no library of the device).
#
#   packaging/build-rust-deb.sh [--with-images] [--revision rev1]
#
# The amiibo thumbnails (artwork owned by Nintendo) are only packaged with --with-images, for a private build;
# the public package never contains them. Needs the aarch64 Rust target and aarch64-linux-gnu-gcc (see rust/.cargo/config.toml).
# Output: dist/ZeroTag_<version>_<revision>_arm64.deb
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
CARGO=${CARGO:-cargo}
REVISION=rev1
WITH_IMAGES=0
while [ $# -gt 0 ]; do
    case "$1" in
        --with-images) WITH_IMAGES=1 ;;
        --revision) shift; REVISION=$1 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
    shift
done

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/rust/Cargo.toml" | head -1)
[ -n "$VERSION" ] || { echo "cannot read the version from rust/Cargo.toml" >&2; exit 1; }
ARCH=arm64
NAME=ZeroTag_${VERSION}_${REVISION}_${ARCH}
STAGE="$ROOT/dist/rust-deb/$NAME"
OUT="$ROOT/dist/$NAME.deb"
MAINTAINER="sukoshi1507 <17269210+sukoshi1507@users.noreply.github.com>"

echo "== building zerotag $VERSION for aarch64"
(cd "$ROOT/rust" && "$CARGO" build --release --target aarch64-unknown-linux-gnu -p zerotag-app)
BIN="$ROOT/rust/target/aarch64-unknown-linux-gnu/release/zerotag"
[ -x "$BIN" ] || { echo "missing $BIN" >&2; exit 1; }

echo "== staging $STAGE"
rm -rf "$STAGE"
install -d -m 755 "$STAGE/DEBIAN" "$STAGE/usr/bin" "$STAGE/etc" "$STAGE/usr/lib/zerotag" \
    "$STAGE/usr/share/zerotag/amiibo" "$STAGE/usr/share/APPLaunch/applications" \
    "$STAGE/usr/share/APPLaunch/share/images" "$STAGE/usr/share/doc/zerotag"
install -m 755 "$BIN" "$STAGE/usr/bin/zerotag"
install -m 644 "$ROOT/config/zerotag.conf" "$STAGE/etc/zerotag.conf"
install -m 755 "$ROOT/packaging/cap-setup" "$STAGE/usr/lib/zerotag/cap-setup"
install -m 644 "$ROOT/assets/amiibo/amiibo_db.json" "$STAGE/usr/share/zerotag/amiibo/amiibo_db.json"
if [ "$WITH_IMAGES" = 1 ]; then
    [ -d "$ROOT/assets/amiibo/images" ] || { echo "no assets/amiibo/images (tools/fetch_amiibo_images.py)" >&2; exit 1; }
    cp -r "$ROOT/assets/amiibo/images" "$STAGE/usr/share/zerotag/amiibo/images"
    find "$STAGE/usr/share/zerotag/amiibo/images" -type d -exec chmod 755 {} + -o -type f -exec chmod 644 {} +
fi
for icon in zerotag.png zerotag_100.png zerotag_80.png; do
    install -m 644 "$ROOT/assets/images/$icon" "$STAGE/usr/share/APPLaunch/share/images/$icon"
done
cat > "$STAGE/usr/share/APPLaunch/applications/zerotag.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=ZeroTag
Comment=Amiibo emulator for CardputerZero (ST25R3916 NFC)
Exec=/usr/bin/zerotag
Icon=share/images/zerotag.png
Terminal=false
Categories=Utility;
StartupNotify=false
DESKTOP
chmod 644 "$STAGE/usr/share/APPLaunch/applications/zerotag.desktop"
install -m 644 "$ROOT/README.md" "$STAGE/usr/share/doc/zerotag/README.md"
install -m 644 "$ROOT/LICENSE" "$STAGE/usr/share/doc/zerotag/copyright"

# No maintainer scripts: the store rejects scripts matching its "dangerous pattern" list.
printf '/etc/zerotag.conf\n' > "$STAGE/DEBIAN/conffiles"
SIZE=$(du -sk --exclude=DEBIAN "$STAGE" | cut -f1)
cat > "$STAGE/DEBIAN/control" <<CONTROL
Package: zerotag
Version: ${VERSION}-${REVISION}
Architecture: ${ARCH}
Maintainer: ${MAINTAINER}
Installed-Size: ${SIZE}
Section: utils
Priority: optional
Depends: fonts-noto-cjk
Recommends: curl, alsa-utils, sudo
Homepage: https://github.com/sukoshi1507/zerotag
Description: Amiibo emulator for CardputerZero (ST25R3916 NFC)
 Amiibo library and emulator for the M5Stack CardputerZero with the Cap CC1101.
 Browse your backups, create blank signed amiibo, scan real figures, emulate
 them on a console and keep what the game writes back. Statically linked Rust
 build: the fonts of fonts-noto-cjk draw the interface, curl is only used by
 the optional catalogue update and aplay (alsa-utils) by the sounds.
CONTROL

echo "== packaging $OUT"
mkdir -p "$ROOT/dist"
dpkg-deb --root-owner-group --build "$STAGE" "$OUT"
dpkg-deb --info "$OUT" | sed -n 1,20p
echo "== done: $OUT ($(du -h "$OUT" | cut -f1))"
