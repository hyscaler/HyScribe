#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Makes the hostile upload cases, crates/hyscribe-image/tests/hostile, from
# the upload corpus: the security review's cases (2026-11-17), read by the
# crate's tests and seeded into the image fuzz targets. The files are
# committed: run it only to change them.
import os
import struct
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CORPUS = os.path.join(ROOT, "crates/hyscribe-image/tests/corpus")
OUT = os.path.join(ROOT, "crates/hyscribe-image/tests/hostile")
os.makedirs(OUT, exist_ok=True)


def save(name, data):
    with open(os.path.join(OUT, name), "wb") as f:
        f.write(data)


with open(os.path.join(CORPUS, "small.jpg"), "rb") as f:
    small = f.read()
# small.jpg: APP0, two DQT, SOF0 at 0x9E (19 bytes), DHT, SOS, scan, EOI.
sof = small[0x9E:0xB1]
tables = small[0x14:0x9E] + small[0xB1:]


def frame(width, height):
    out = bytearray(sof)
    out[5:7] = height.to_bytes(2, "big")
    out[7:9] = width.to_bytes(2, "big")
    return bytes(out)


# FF 00: zune-jpeg skips it as fill and reads the 2000x2000 frame after it;
# PHP's marker walk takes 00 as a marker whose length is FF C0 (65,472) and
# lands on the 16x16 frame hidden in a comment.
jpeg = bytearray(b"\xFF\xD8\xFF\x00" + frame(2000, 2000))
land = 4 + 0xFFC0
comment = land + len(sof) + 8 - (len(jpeg) + 2)
body = bytearray(comment - 2)
at = land - (len(jpeg) + 4)
body[at : at + len(sof)] = frame(16, 16)
jpeg += b"\xFF\xFE" + comment.to_bytes(2, "big") + body + tables
save("jpeg-ff00.jpg", bytes(jpeg))


def chunk(kind, data):
    crc = zlib.crc32(kind + data) & 0xFFFFFFFF
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", crc)


# A 1x1 PNG whose iCCP inflates to 17 MiB.
profile = zlib.compress(b"\0" * (17 << 20), 9)
png = (
    b"\x89PNG\r\n\x1a\n"
    + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0))
    + chunk(b"iCCP", b"icc\0\0" + profile)
    + chunk(b"IDAT", zlib.compress(b"\0\0\0\0"))
    + chunk(b"IEND", b"")
)
save("png-iccp.png", png)

# EXIF whose ImageDescription lies past its APP1, in the bytes after it.
text = b"Past the segment\0"
ifd = b"MM\0*\0\0\0\x08" + struct.pack(">H", 1)
ifd += struct.pack(">HHII", 0x010E, 2, len(text), 8 + 2 + 12 + 4) + b"\0\0\0\0"
app1 = b"Exif\0\0" + ifd
save(
    "exif-past-app1.jpg",
    b"\xFF\xD8\xFF\xE1" + struct.pack(">H", len(app1) + 2) + app1 + text + small[2:],
)

# An APP13 of 13,106 datasets with distinct keys and no values.
records = (1, 2) + tuple(range(3, 256)) + (0,)
keys = [(r, d) for r in records for d in range(256)][:13106]
iim = b"".join(b"\x1C" + bytes([r, d]) + b"\0\0" for r, d in keys)
app13 = b"\xFF\xED" + struct.pack(">H", len(iim) + 2) + iim
save("iptc-keys.jpg", small[:2] + app13 + small[2:])
