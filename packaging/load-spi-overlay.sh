#!/bin/sh
# SPDX-License-Identifier: MIT
# Loads the device-tree overlay that exposes /dev/spidev0.2, the SPI port of the Cap CC1101 (ST25R3916 NFC).
# The overlay is not persistent across reboots and needs root, hence this boot-time helper.
OVERLAY=spi0-spidev2-gpio22-overlay
NODE=/dev/spidev0.2

[ -e "$NODE" ] && exit 0

if ! dtoverlay -l 2>/dev/null | grep -q "$OVERLAY"; then
    dtoverlay "$OVERLAY" || exit 1
fi

# udev creates the node (group spi) a moment later.
i=0
while [ ! -e "$NODE" ] && [ "$i" -lt 50 ]; do
    sleep 0.1
    i=$((i + 1))
done
[ -e "$NODE" ]
