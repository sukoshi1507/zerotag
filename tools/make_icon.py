#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Draws the ZeroTag launcher icon (a Famicom-style cartridge, 20x20 pixel art) and writes it at the sizes
used by assets/images/zerotag*.png. Pure standard library (zlib + struct), no image library needed.

usage: make_icon.py <output_dir>
"""
import struct
import sys
import zlib

PALETTE = {
    ".": None,
    "K": (0x1D, 0x1B, 0x22, 255),  # outline
    "W": (0xF4, 0xEE, 0xDD, 255),  # cream body
    "R": (0xD3, 0x26, 0x2F, 255),  # Famicom red
    "D": (0x8E, 0x18, 0x20, 255),  # dark red label border
    "G": (0xA9, 0xA3, 0x93, 255),  # grey ridges / connector
    "Y": (0xF3, 0xC9, 0x3A, 255),  # gold (triforce)
}

LABEL = [  # 12 x 7, inside the red label
    "RRRRRRRRRRRR",
    "RRRRRRYRRRRR",
    "RRRRRYYYRRRR",
    "RRRRYRRRYRRR",
    "RRRYYYRYYYRR",
    "RRRRRRRRRRRR",
    "RRWWWWWWWWRR",
]

ROWS = [
    "..KKKKKKKKKKKKKKKK..",
    ".KWWWWWWWWWWWWWWWWK.",
    ".KW" + "D" * 14 + "WK.",
]
ROWS += [".KWD" + row + "DWK." for row in LABEL]
ROWS += [
    ".KW" + "D" * 14 + "WK.",
    ".KWWWWWWWWWWWWWWWWK.",
    ".KW" + "GWGWGWGWGWGWGW" + "WK.",
    ".KW" + "GWGWGWGWGWGWGW" + "WK.",
    ".KWWWWWWWWWWWWWWWWK.",
    "..KKKKKKKKKKKKKKKK..",
    "...K" + "GGGGGGGGGGGG" + "K...",
    "...K" + "GGKGGGGGGKGG" + "K...",
    "...K" + "K" * 12 + "K...",
    "." * 20,
]
assert len(ROWS) == 20 and all(len(r) == 20 for r in ROWS), [len(r) for r in ROWS]


def png(width, height, pixels):
    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    raw = b"".join(b"\x00" + bytes(v for px in row for v in px) for row in pixels)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)) +
            chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def render(scale):
    out = []
    for row in ROWS:
        line = []
        for ch in row:
            color = PALETTE[ch] or (0, 0, 0, 0)
            line.extend([color] * scale)
        out.extend([line] * scale)
    return png(20 * scale, 20 * scale, out)


if __name__ == "__main__":
    target = sys.argv[1] if len(sys.argv) > 1 else "."
    for name, scale in (("zerotag.png", 5), ("zerotag_100.png", 5), ("zerotag_80.png", 4)):
        with open(f"{target}/{name}", "wb") as f:
            f.write(render(scale))
        print("wrote", name, 20 * scale)
