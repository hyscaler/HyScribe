# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The MP3 files reference/getid3/golden.sh asks getID3 about: usage
# `mp3-samples.py <out>`. Hand-built frames (headers over silence) in every
# shape the reader takes or leaves: CBR and VBR streams, Xing, Info and VBRI
# headers, LAME tags, ID3v1 and ID3v2 tags, embedded pictures, and cut or
# corrupt files. Deterministic; nothing here is a recording.
import os
import random
import struct
import sys

out = sys.argv[1]
os.makedirs(out, exist_ok=True)
rng = random.Random(20261124)

RATES = {
    (1, 1): [32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448],
    (1, 2): [32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384],
    (1, 3): [32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320],
    (2, 1): [32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256],
    (2, 2): [8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
}
RATES[(2, 3)] = RATES[(2, 2)]
FREQ = {1: [44100, 48000, 32000], 2: [22050, 24000, 16000], 25: [11025, 12000, 8000]}


def put(name, data):
    with open(os.path.join(out, name), 'wb') as f:
        f.write(data)


def header(version=1, layer=3, kbps=128, rate=44100, mode=1, pad=0, prot=1):
    vbits = {1: 3, 2: 2, 25: 0}[version]
    lbits = {1: 3, 2: 2, 3: 1}[layer]
    table = RATES[(1 if version == 1 else 2, layer)]
    b = 0 if kbps == 0 else table.index(kbps) + 1
    r = FREQ[version].index(rate)
    return bytes([0xff, 0xe0 | (vbits << 3) | (lbits << 1) | prot, (b << 4) | (r << 2) | (pad << 1), mode << 6])


def flen(version, layer, kbps, rate, pad):
    bitrate = kbps * 1000
    if version == 1:
        coef, slot = (48, 4) if layer == 1 else (144, 1)
    elif layer == 1:
        coef, slot = 24, 4
    elif layer == 2:
        coef, slot = 144, 1
    else:
        coef, slot = 72, 1
    n = (coef * bitrate // rate) // slot * slot
    return n + (slot if pad else 0)


def frame(version=1, layer=3, kbps=128, rate=44100, mode=1, pad=0, prot=1, fill=0):
    n = flen(version, layer, kbps, rate, pad)
    return (header(version, layer, kbps, rate, mode, pad, prot) + bytes([fill]) * (n - 4))


def stream(count, version=1, layer=3, kbps=128, rate=44100, mode=1, pads=True):
    """count frames of a constant bitrate, padded as an encoder would."""
    coef = {(1, 1): 12, (1, 2): 144, (1, 3): 144, (2, 1): 12, (2, 2): 144, (2, 3): 72}[(1 if version == 1 else 2, layer)]
    exact = coef * kbps * 1000 / rate
    frames, carry = [], 0.0
    for _ in range(count):
        carry += exact - int(exact)
        pad = 0
        if pads and carry >= 1.0:
            pad, carry = 1, carry - 1.0
        frames.append(frame(version, layer, kbps, rate, mode, pad))
    return b''.join(frames)


def be32(n):
    return struct.pack('>I', n)


def info_frame(tag=b'Info', version=1, kbps=128, rate=44100, mode=1, frames=None, size=None, toc=False,
               scale=None, lame=None, extra=b'', flags=None, layer=3):
    """The first frame of a stream: a Xing or Info header, and a LAME tag."""
    n = flen(version, layer, kbps, rate, 0)
    f = bytearray(header(version, layer, kbps, rate, mode)) + bytearray(n - 4)
    mono = mode == 3
    idoff = {1: 0x15 if mono else 0x24, 2: 0x0d if mono else 0x15, 25: 0x15}[version]
    if flags is None:
        flags = (1 if frames is not None else 0) | (2 if size is not None else 0) | (4 if toc else 0) | (8 if scale is not None else 0)
    body = tag + be32(flags)
    if frames is not None:
        body += be32(frames)
    if size is not None:
        body += be32(size)
    if toc:
        body += bytes(range(100))
    if scale is not None:
        body += be32(scale)
    f[idoff:idoff + len(body)] = body
    if lame is not None:
        f[idoff + 119:idoff + 119 + len(lame)] = lame
    f[idoff + 120:idoff + 120 + len(extra)] = extra
    return bytes(f)


def lame_tag(version=b'LAME3.99r', quality=0, method=3, lowpass=0, flags=0, minbr=0, misc=0, preset=0, audio=0):
    """The 36 bytes of a LAME tag from the VBR scale's low byte on."""
    t = bytearray()
    t += bytes([quality])
    t += version.ljust(9, b'\0')[:9]
    t += bytes([(0 << 4) | method])
    t += bytes([lowpass // 100])
    t += be32(0)
    t += struct.pack('>HH', 0, 0)
    t += bytes([flags, minbr])
    t += bytes([0, 0, 0])
    t += bytes([misc, 0])
    t += struct.pack('>H', preset)
    t += be32(audio)
    t += struct.pack('>HH', 0, 0)
    return bytes(t)


def vbri_frame(version=1, kbps=128, rate=44100, mode=1, quality=75, size=0, frames=0):
    n = flen(version, 3, kbps, rate, 0)
    f = bytearray(header(version, 3, kbps, rate, mode)) + bytearray(n - 4)
    f[36:36 + 26] = b'VBRI' + struct.pack('>HHHIIHHHH', 1, 0, quality, size, frames, 0, 1, 2, 1)
    return bytes(f)


# ID3v2 tags.
def synchsafe(n):
    return bytes([(n >> 21) & 0x7f, (n >> 14) & 0x7f, (n >> 7) & 0x7f, n & 0x7f])


def v2frame(major, name, data, flags=0):
    size = len(data)
    sz = be32(size) if major == 3 else synchsafe(size)
    return name + sz + struct.pack('>H', flags) + data


def text(major, name, value, enc=0, flags=0):
    if enc == 0:
        data = bytes([0]) + value.encode('latin-1')
    elif enc == 1:
        data = bytes([1]) + b'\xff\xfe' + value.encode('utf-16-le')
    elif enc == 2:
        data = bytes([2]) + value.encode('utf-16-be')
    else:
        data = bytes([3]) + value.encode('utf-8')
    return v2frame(major, name, data, flags)


def term(enc):
    return b'\0\0' if enc in (1, 2) else b'\0'


def enc_bytes(enc, value):
    if enc == 0:
        return value.encode('latin-1')
    if enc == 1:
        return b'\xff\xfe' + value.encode('utf-16-le') if value else b'\xff\xfe'
    if enc == 2:
        return value.encode('utf-16-be')
    return value.encode('utf-8')


def comm(major, desc, value, enc=0, lang=b'eng'):
    data = bytes([enc]) + lang + enc_bytes(enc, desc) + term(enc) + enc_bytes(enc, value)
    return v2frame(major, b'COMM', data)


def txxx(major, desc, value, enc=0):
    data = bytes([enc]) + enc_bytes(enc, desc) + term(enc) + enc_bytes(enc, value)
    return v2frame(major, b'TXXX', data)


def apic(major, image, mime=b'image/jpeg', ptype=3, desc=b'', enc=0):
    data = bytes([enc]) + mime + b'\0' + bytes([ptype]) + desc + term(enc) + image
    return v2frame(major, b'APIC', data)


def v2tag(major, frames, pad=0, flags=0, ext=b'', unsync=False):
    body = ext + b''.join(frames) + bytes(pad)
    if unsync:
        body = body.replace(b'\xff', b'\xff\x00')
        flags |= 0x80
    return b'ID3' + bytes([major, 0, flags]) + synchsafe(len(body)) + body


def v1tag(title=b'', artist=b'', album=b'', year=b'', comment=b'', track=None, genre=255):
    c = comment.ljust(30, b'\0')[:30]
    if track is not None:
        c = comment.ljust(28, b'\0')[:28] + b'\0' + bytes([track])
    return (b'TAG' + title.ljust(30, b'\0')[:30] + artist.ljust(30, b'\0')[:30] + album.ljust(30, b'\0')[:30]
            + year.ljust(4, b'\0')[:4] + c + bytes([genre]))


# Pictures: a 2x2 PNG, GIF and JPEG made by hand are not needed; the files
# below are tiny but real.
def png(w=2, h=2):
    import zlib

    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body) & 0xffffffff)
    raw = b''.join(b'\0' + b'\x80\x40\x20' * w for _ in range(h))
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b'')


def gif():
    return b'GIF89a\x02\x00\x02\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x02\x00\x02\x00\x00\x02\x02D\x01\x00;'


def jpeg():
    # A baseline JPEG header with an SOF0 of 3x5, no scan: enough for its size.
    return (b'\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00'
            b'\xff\xdb\x00\x43\x00' + bytes([8] * 64) +
            b'\xff\xc0\x00\x0b\x08\x00\x05\x00\x03\x01\x01\x11\x00'
            b'\xff\xc4\x00\x14\x00\x01' + bytes(15) + b'\x00' +
            b'\xff\xda\x00\x08\x01\x01\x00\x00\x3f\x00\x7f\xff\xd9')


AUDIO = stream(12)
AUDIO22 = stream(12, version=2, kbps=64, rate=22050, mode=3)

# Constant bitrate streams without a header.
put('mp3-cbr128-44k.mp3', AUDIO)
put('mp3-cbr128-44k-nopad.mp3', stream(12, pads=False))
put('mp3-cbr320-48k.mp3', stream(12, kbps=320, rate=48000, mode=0))
put('mp3-cbr64-22k-mono.mp3', AUDIO22)
put('mp3-cbr32-11k-mono.mp3', stream(40, version=25, kbps=32, rate=11025, mode=3))
put('mp3-cbr8-22k-80frames.mp3', stream(80, version=2, kbps=8, rate=22050, mode=3))
put('mp3-cbr8-22k-few.mp3', stream(30, version=2, kbps=16, rate=22050, mode=3))
put('mp3-layer2-192.mp3', stream(12, layer=2, kbps=192))
put('mp3-layer2-mono-64.mp3', stream(12, layer=2, kbps=64, mode=3))
put('mp3-layer2-bad-combo.mp3', stream(12, layer=2, kbps=56))
put('mp3-layer2-mono-too-high.mp3', stream(12, layer=2, kbps=224, mode=3))
put('mp3-layer1-256.mp3', stream(12, layer=1, kbps=256))
put('mp3-layer1-mpeg2.mp3', stream(12, version=2, layer=1, kbps=96, rate=24000))
put('mp3-free-format.mp3', header(1, 3, 0) + bytes(417 - 4) + header(1, 3, 0) + bytes(417 - 4) + bytes(700))
put('mp3-crc-protected.mp3', b''.join(frame(prot=0) for _ in range(12)))
put('mp3-dual-channel.mp3', stream(12, mode=2))
put('mp3-mpeg2-48.mp3', stream(12, version=2, kbps=96, rate=24000))
put('mp3-mpeg2-16.mp3', stream(12, version=2, kbps=64, rate=16000))
put('mp3-mpeg25-8.mp3', stream(30, version=25, kbps=16, rate=8000, mode=3))
put('mp3-short-600.mp3', stream(2, kbps=160, rate=32000)[:600].ljust(600, b'\0'))
put('mp3-too-small.mp3', stream(1, kbps=32, rate=32000)[:300])
put('mp3-trailing-zeros.mp3', AUDIO + bytes(600))
put('mp3-truncated-frame.mp3', AUDIO[:-100])
put('mp3-truncated-header.mp3', AUDIO[:len(AUDIO) - 417 + 2])
put('mp3-one-frame.mp3', stream(1, kbps=320).ljust(1200, b'\0'))
put('mp3-garbage-first.mp3', b'\0' * 100 + AUDIO)
put('mp3-garbage-ff.mp3', b'\xff' * 4 + bytes(200) + AUDIO)
put('mp3-bad-second.mp3', frame() + bytes(417) + frame() * 5)
put('mp3-vbr-no-header.mp3', b''.join(frame(kbps=k) for k in (128, 192, 96, 160, 128, 256, 64, 112, 128, 320, 128, 96)))
put('mp3-switch-rate.mp3', frame() * 6 + frame(rate=48000) * 6)
put('mp3-switch-layer.mp3', frame() * 6 + frame(layer=2) * 6)
put('mp3-xing-in-later-frame.mp3', frame() * 4 + info_frame() + frame() * 6)
put('mp3-empty-after-id3.mp3', v2tag(3, [text(3, b'TIT2', 'T')], pad=600))

# Xing, Info and VBRI headers.
body = stream(11)
put('mp3-info-cbr.mp3', info_frame(b'Info', frames=12, size=len(body) + 417) + body)
put('mp3-info-nolame-lavf.mp3', info_frame(b'Info', frames=12, size=len(body) + 417, extra=b'Lavf58.76.100') + body)
put('mp3-xing-vbr.mp3', info_frame(b'Xing', frames=1000, size=len(body) + 417, toc=True, scale=75) + body)
put('mp3-xing-vbr-framesonly.mp3', info_frame(b'Xing', frames=500) + body)
put('mp3-xing-noflags.mp3', info_frame(b'Xing', flags=0) + body)
put('mp3-xing-bytes-only.mp3', info_frame(b'Xing', size=len(body) + 417) + body)
put('mp3-xing-frames-zero.mp3', info_frame(b'Xing', frames=0, size=12345) + body)
put('mp3-xing-nobytes-fallback.mp3', info_frame(b'Xing', frames=12) + body)
put('mp3-xing-bytes-zero.mp3', info_frame(b'Xing', frames=12, size=0) + body)
put('mp3-xing-bytes-one-less.mp3', info_frame(b'Xing', frames=12, size=len(body) + 416) + body)
put('mp3-xing-bytes-one-more.mp3', info_frame(b'Xing', frames=12, size=len(body) + 418) + body)
put('mp3-xing-bytes-short.mp3', info_frame(b'Xing', frames=12, size=len(body) + 4000) + body)
put('mp3-xing-bytes-long.mp3', info_frame(b'Xing', frames=12, size=len(body) - 4000) + body)
mono = stream(11, version=2, kbps=64, rate=22050, mode=3)
put('mp3-info-mono-mpeg2.mp3', info_frame(b'Info', version=2, kbps=64, rate=22050, mode=3, frames=12, size=len(mono) + 208) + mono)
st2 = stream(11, version=2, kbps=64, rate=22050, mode=1)
put('mp3-info-stereo-mpeg2.mp3', info_frame(b'Info', version=2, kbps=64, rate=22050, mode=1, frames=12, size=len(st2) + 208) + st2)
m25 = stream(11, version=25, kbps=32, rate=11025, mode=3)
put('mp3-info-mpeg25.mp3', info_frame(b'Info', version=25, kbps=32, rate=11025, mode=3, frames=12, size=len(m25) + 104) + m25)
put('mp3-info-mono-mpeg1.mp3', info_frame(b'Info', kbps=64, mode=3, frames=12, size=len(stream(11, kbps=64, mode=3)) + 208) + stream(11, kbps=64, mode=3))
vb = b''.join(frame(kbps=k) for k in (128, 192, 96, 160, 128, 256, 64, 112, 128, 320, 128))
put('mp3-xing-vbr-real.mp3', info_frame(b'Xing', frames=12, size=len(vb) + 417, toc=True) + vb)
put('mp3-xing-vbr-close.mp3', info_frame(b'Xing', frames=12, size=len(body) + 417 + 10) + body)
put('mp3-xing-vbr-far.mp3', info_frame(b'Xing', frames=24, size=len(body) + 417) + body)
l1 = stream(11, layer=1, kbps=256)
put('mp3-xing-layer1.mp3', info_frame(b'Xing', frames=12, size=len(l1) + 0) + l1)
put('mp3-vbri.mp3', vbri_frame(size=len(body) + 417, frames=12) + body)
put('mp3-vbri-zero.mp3', vbri_frame(size=0, frames=0) + body)
put('mp3-vbri-quality0.mp3', vbri_frame(quality=0, size=len(body) + 417, frames=12) + body)
put('mp3-vbri-bytes-one-less.mp3', vbri_frame(size=len(body) + 416, frames=12) + body)

# LAME tags.
VERSIONS = [b'LAME3.99r', b'LAME3.100', b'LAME3.98.2', b'LAME3.90.', b'LAME3.94a', b'LAME3.93.1', b'LAME3.88', b'LAME3.89', b'LAME3.9', b'LAMEx']
for i, v in enumerate(VERSIONS):
    put('mp3-lame-version-%d.mp3' % i, info_frame(b'Info', frames=12, size=len(body) + 417, lame=lame_tag(v, 0, 1, 19500, flags=0x02)) + body)
for method in range(0, 16):
    put('mp3-lame-method-%d.mp3' % method, info_frame(b'Xing', frames=12, size=len(body) + 417, lame=lame_tag(b'LAME3.99r', 78, method, 19500, flags=0x02, minbr=32 if method else 0, misc=0x0b)) + body)
combos = 0
for rate, version, mode, kbps in ((44100, 1, 1, 128), (48000, 1, 1, 128), (32000, 1, 1, 96), (22050, 2, 3, 64), (16000, 2, 3, 64), (11025, 25, 3, 32)):
    audio = stream(11, version=version, kbps=kbps, rate=rate, mode=mode)
    for src in range(4):
        for preset in (0, 1001, 1004, 1007, 410, 480, 460, 500, 1015, 1010, 320, 128, 300, 7777):
            for method, quality, lowpass in ((1, 0, 0), (2, 0, 17000), (3, 78, 19000), (4, 78, 19000), (4, 58, 20500), (3, 57, 16000)):
                if rng.random() > 0.03:
                    continue
                misc = (src << 6) | (1 << 2) | 2
                tag = lame_tag(b'LAME3.%s' % rng.choice([b'99r', b'100', b'94a', b'90.', b'96']), quality, method, lowpass, flags=rng.choice([0, 2, 4, 0x40, 0x80]), minbr=rng.choice([0, 32, 128, 192, 255]), misc=misc, preset=preset)
                put('mp3-lame-combo-%03d.mp3' % combos, info_frame(rng.choice([b'Xing', b'Info']), version=version, kbps=kbps, rate=rate, mode=mode, frames=12, size=len(audio) + flen(version, 3, kbps, rate, 0), lame=tag) + audio)
                combos += 1
KNOWN = [
    (255, 58, 1, 1, 3, 2, 20500), (None, 78, 3, 2, 3, 2, 19500), (None, 78, 4, 1, 3, 4, 19000), (None, 88, 4, 1, 3, 3, 19600),
    (None, 67, 4, 1, 3, 4, 18000), (None, 68, 3, 2, 3, 4, 18000), (255, 99, 1, 1, 1, 2, 0), (192, 58, 2, 2, 3, 2, 19600),
    (160, 78, 1, 1, 3, 2, 18000), (128, 67, 1, 1, 3, 2, 15000), (112, 57, 2, 1, 3, 4, 16000), (56, 58, 2, 2, 0, 2, 10000),
    (40, 57, 2, 1, 0, 4, 8800), (24, 58, 2, 2, 0, 2, 3900), (16, 57, 2, 1, 0, 4, 5600),
]
for i, (minbr, q, method, ns, stereo, ath, lowpass) in enumerate(KNOWN):
    misc = ns | (stereo << 2) | (1 << 6)
    tag = lame_tag(b'LAME3.94b', q, method, lowpass, flags=ath, minbr=minbr if minbr is not None else 77, misc=misc)
    put('mp3-lame-known-%02d.mp3' % i, info_frame(b'Xing', frames=12, size=len(body) + 417, lame=tag) + body)
# Layer 1 streams with a Xing header, and a LAME CBR tag past 320 kbit/s.
l1x = stream(11, layer=1, kbps=256)
put('mp3-xing-layer1-frames.mp3', info_frame(b'Xing', layer=1, kbps=256, frames=24, size=len(l1x) + flen(1, 1, 256, 44100, 0)) + l1x)
l448 = stream(11, layer=1, kbps=448)
put('mp3-lame-cbr-448.mp3', info_frame(b'Info', layer=1, kbps=448, frames=12, size=len(l448) + flen(1, 1, 448, 44100, 0), lame=lame_tag(b'LAME3.99r', 0, 1, 0, misc=1 << 6)) + l448)
# Every preset name, plain and fast; a version without digits; the audio
# byte count of the tag; quality without a known preset; empty options.
for preset in (8, 200, 410, 420, 430, 440, 450, 460, 470, 480, 490, 500, 1000, 1001, 1002, 1003, 1004, 1005, 1006, 1007, 1010, 1015, 7777):
    for method in (3, 4):
        put('mp3-lame-preset-%d-%d.mp3' % (preset, method), info_frame(b'Xing', frames=24, size=len(body) + 417, lame=lame_tag(b'LAME3.99r', 78, method, 0, misc=1 << 6, preset=preset)) + body)
put('mp3-lame-version-bare.mp3', info_frame(b'Info', frames=12, size=len(body) + 417, lame=lame_tag(b'LAME 3.99', 0, 1, 19500, flags=0x02)) + body)
put('mp3-lame-audio-bytes.mp3', info_frame(b'Info', frames=12, size=len(body) + 417, lame=lame_tag(b'LAME3.99r', 0, 1, 19500, flags=0x02, audio=len(body) + 417)) + body)
put('mp3-lame-quality-unknown.mp3', info_frame(b'Xing', frames=24, size=len(body) + 417, lame=lame_tag(b'LAME3.99r', 50, 3, 0, misc=1 << 6)) + body)
put('mp3-lame-options-empty.mp3', info_frame(b'Xing', frames=24, size=len(body) + 417, lame=lame_tag(b'LAME3.99r', 78, 7, 0, misc=1 << 6)) + body)
put('mp3-v24-utf16be.mp3', v2tag(4, [text(4, b'TIT2', 'Big end', 2)], pad=8) + AUDIO)
put('mp3-v23-exthead-no-padding.mp3', v2tag(3, [], pad=0, flags=0x40, ext=b'\0\0\0\x06\0\0') + AUDIO)
put('mp3-v24-exthead-no-flags.mp3', v2tag(4, [], pad=0, flags=0x40, ext=synchsafe(6) + b'\x02') + AUDIO)
put('mp3-v24-exthead-no-crc-length.mp3', v2tag(4, [], pad=0, flags=0x40, ext=synchsafe(6) + b'\x01\x20') + AUDIO)
put('mp3-v1-before-only.mp3', AUDIO + v1tag(b'Old') + bytes(128))
put('mp3-rate-reserved.mp3', b'\xff\xfb\x9c\x40' + bytes(413) + AUDIO)
put('mp3-free-later.mp3', frame() * 3 + header(1, 3, 0) + bytes(413) + frame() * 8)
put('mp3-header-frame-only.mp3', v2tag(3, [text(3, b'TIT2', 'Alone')], pad=300) + info_frame(b'Info', frames=1, size=417))
put('mp3-v24-utf16be-zero.mp3', v2tag(4, [v2frame(4, b'TIT2', b'\x02\x000')], pad=8) + AUDIO)
put('mp3-v23-apic-gif-no-height.mp3', v2tag(3, [apic(3, b'GIF89a\x02\x00\x00\x00\x80\x00\x00' + gif()[13:], b'image/gif')], pad=8) + AUDIO)
put('mp3-v24-exthead-eight-flags.mp3', v2tag(4, [text(4, b'TIT2', 'Song')], pad=8, flags=0x40, ext=synchsafe(13) + b'\x08' + bytes(8)) + AUDIO)
put('mp3-v24-exthead-nine-flags.mp3', v2tag(4, [text(4, b'TIT2', 'Song')], pad=8, flags=0x40, ext=synchsafe(14) + b'\x09' + b'\xff' * 9) + AUDIO)
# Frame headers and the scan: a sync missing, layer 1 of MPEG 2 with padding,
# a later VBRI header, a frame's length of bytes left, a header cut short.
put('mp3-sync-missing.mp3', b'\xff\x1b\x90\x40' + bytes(413) + AUDIO)
put('mp3-layer1-mpeg2-22k.mp3', stream(12, version=2, layer=1, kbps=96, rate=22050))
put('mp3-vbri-in-later-frame.mp3', frame() * 4 + vbri_frame(size=5000, frames=12) + frame() * 6)
put('mp3-frame-of-zeros-after.mp3', AUDIO + bytes(417))
put('mp3-header-cut-after.mp3', AUDIO + b'\xff\xfb\xa0')
put('mp3-exactly-512.mp3', stream(2)[:512])
put('mp3-audio-226.mp3', v2tag(3, [text(3, b'TIT2', 'Short')], pad=300) + frame()[:226])
# Bitrates: an integer one, a LAME CBR at 320 kbit/s, a Xing header's bytes
# not flagged, presets and qualities GuessEncoderOptions() names.
body12 = stream(11)
xi = info_frame(b'Xing', frames=12)
put('mp3-xing-integer-bitrate.mp3', (xi + body12)[:416 * 12])
put('mp3-xing-frames-no-bytes-v1.mp3', xi + body12 + v1tag(b'Tagged'))
put('mp3-xing-toc-no-bytes.mp3', info_frame(b'Xing', frames=24, toc=True) + body12)
b320 = stream(11, kbps=320)
put('mp3-lame-cbr-320.mp3', info_frame(b'Info', kbps=320, frames=12, size=len(b320) + flen(1, 3, 320, 44100, 0), lame=lame_tag(b'LAME3.99r', 0, 1, 0, misc=1 << 6)) + b320)
put('mp3-lame-preset-200-1.mp3', info_frame(b'Info', frames=12, size=len(body) + 417, lame=lame_tag(b'LAME3.99r', 0, 1, 0, misc=1 << 6, preset=200)) + body)
put('mp3-lame-quality-57.mp3', info_frame(b'Xing', frames=24, size=len(body) + 417, lame=lame_tag(b'LAME3.99r', 57, 3, 0, misc=1 << 6)) + body)
b48 = stream(11, rate=48000)
put('mp3-lame-lowpass-22000.mp3', info_frame(b'Xing', rate=48000, frames=24, size=len(b48) + flen(1, 3, 128, 48000, 0), lame=lame_tag(b'LAME3.99r', 78, 3, 22000, misc=2 << 6, preset=500)) + b48)
b32 = stream(11, kbps=96, rate=32000)
voice = lame_tag(b'LAME3.94b', 58, 2, 10000, flags=2, minbr=56, misc=2 | (1 << 6))
put('mp3-lame-voice-32k.mp3', info_frame(b'Xing', kbps=96, rate=32000, frames=12, size=len(b32) + flen(1, 3, 96, 32000, 0), lame=voice) + b32)
b22 = stream(11, version=2, kbps=64, rate=22050)
put('mp3-lame-voice-22k.mp3', info_frame(b'Xing', version=2, kbps=64, rate=22050, frames=12, size=len(b22) + flen(2, 3, 64, 22050, 0), lame=voice) + b22)
end_marked = bytearray(body)
end_marked[-300:-300 + 10] = b'LAME3.99 \xff'
put('mp3-lame-longer-same-length.mp3', info_frame(b'Info', frames=12, size=len(body) + 417, lame=lame_tag(b'LAME3.99a', 0, 1, 19500, flags=2)) + bytes(end_marked))
# Lengths half a second from rounding, where a byte or an ID3v1 tag more or
# less changes the length WordPress keeps.
half = stream(19)
half = half + bytes(8000 - len(half))
put('mp3-half-second.mp3', half)
put('mp3-half-second-v1.mp3', half + v1tag(b'Half'))
put('mp3-half-second-v1-twice.mp3', half + v1tag(b'Old') + v1tag(b'Half'))
short = half[:7999]
put('mp3-under-half-second-v1.mp3', short + v1tag(b'Half'))
put('mp3-under-half-second-v1-twice.mp3', short + v1tag(b'Old') + v1tag(b'Half'))
v2small = v2tag(3, [text(3, b'TIT2', 'Tagged')], pad=20)
put('mp3-v23-xing-frames-no-bytes-v1.mp3', v2small + info_frame(b'Xing', frames=12) + stream(11) + v1tag(b'Tagged'))
def lame_half(total, audio_bytes, xing_bytes):
    tag = lame_tag(b'LAME3.99r', 0, 1, 0, misc=1 << 6, audio=audio_bytes)
    first = info_frame(b'Info', frames=20, size=xing_bytes, lame=tag)
    rest = stream(19)
    return (first + rest + bytes(total))[:total]
put('mp3-lame-one-byte-over.mp3', lame_half(8417, 8416, 8340))
put('mp3-lame-bytes-not-one-over.mp3', lame_half(8417, 8422, 8340))
put('mp3-lame-xing-one-byte-over.mp3', lame_half(8421, 0, 8420))
put('mp3-lame-bytes-three-short.mp3', lame_half(8417, 8414, 8340))
put('mp3-v23-lame-one-byte-over.mp3', v2small + lame_half(8417, 8416, 8340))
l1one = frame(layer=1, kbps=256)
put('mp3-layer1-bad-second.mp3', l1one + bytes(len(l1one)) + stream(8, layer=1, kbps=256))
b144 = stream(11, version=2, kbps=144, rate=22050)
put('mp3-lame-cbr-144.mp3', info_frame(b'Info', version=2, kbps=144, rate=22050, frames=12, size=len(b144) + flen(2, 3, 144, 22050, 0), lame=lame_tag(b'LAME3.99r', 0, 1, 0, misc=1 << 6)) + b144)
b176 = stream(11, version=2, layer=1, kbps=176, rate=22050)
put('mp3-lame-cbr-176.mp3', info_frame(b'Info', version=2, layer=1, kbps=176, rate=22050, frames=12, size=len(b176) + flen(2, 1, 176, 22050, 0), lame=lame_tag(b'LAME3.99r', 0, 1, 0, misc=1 << 6)) + b176)
put('mp3-version-reserved.mp3', b'\xff\xeb\x90\x40' + bytes(413) + AUDIO)
# The LAME version looked for at the end of the file.
def marked(text):
    end = bytearray(body)
    end[-300:-300 + len(text)] = text
    return bytes(end)


tagv = lame_tag(b'LAME3.99r', 0, 1, 19500, flags=2)
put('mp3-lame-longer.mp3', info_frame(b'Info', frames=12, size=len(body) + 417, lame=tagv) + marked(b'LAME3.99.5 (alpha)'))
put('mp3-lame-longer-short-file.mp3', info_frame(b'Info', frames=3, size=1000, lame=tagv) + stream(1, kbps=128)[:300] + bytes(100))
put('mp3-lame-longer-absent.mp3', info_frame(b'Info', frames=12, size=len(body) + 417, lame=tagv) + body)
put('mp3-lame-longer-shorter.mp3', info_frame(b'Info', frames=12, size=len(body) + 417, lame=tagv) + marked(b'LAME3.99r'))
put('mp3-lame-3.99a-end.mp3', info_frame(b'Info', frames=12, size=len(body) + 417, lame=lame_tag(b'LAME3.99a', 0, 1, 19500, flags=2)) + marked(b'LAME3.99 (beta)'))
put('mp3-lame-3.99b-end.mp3', info_frame(b'Info', frames=12, size=len(body) + 417, lame=lame_tag(b'LAME3.99b', 0, 1, 19500, flags=2)) + marked(b'LAME3.99.1'))
put('mp3-lame-old-end.mp3', info_frame(b'Info', frames=12, size=len(body) + 417, lame=lame_tag(b'LAME3.88', 0, 1, 19500, flags=2)) + marked(b'LAME3.88 (beta 1, Sep 27 2000)'))

# ID3v1.
put('mp3-v1.mp3', AUDIO + v1tag(b'Title', b'Artist', b'Album', b'2001', b'A comment', genre=17))
put('mp3-v1-track.mp3', AUDIO + v1tag(b'Title', b'Artist', b'Album', b'2001', b'A comment', track=7, genre=13))
put('mp3-v1-empty.mp3', AUDIO + v1tag())
put('mp3-v1-genre-unknown.mp3', AUDIO + v1tag(b'T', genre=200))
put('mp3-v1-genre-255.mp3', AUDIO + v1tag(b'T', genre=255))
put('mp3-v1-genre-scmpx.mp3', AUDIO + v1tag(b'T', genre=245))
put('mp3-v1-latin1.mp3', AUDIO + v1tag('Caf\xe9'.encode('latin-1'), 'Bj\xf6rk'.encode('latin-1'), b'\xff\xfe'))
put('mp3-v1-markup.mp3', AUDIO + v1tag(b'<b>bold</b><script>x</script>', b'a & b'))
put('mp3-v1-duplicate.mp3', AUDIO + v1tag(b'one') + v1tag(b'two'))
put('mp3-v1-with-info.mp3', info_frame(b'Info', frames=12, size=len(body) + 417) + body + v1tag(b'Title'))
put('mp3-v1-bytes-one-less.mp3', info_frame(b'Xing', frames=12, size=len(body) + 416) + body + v1tag(b'Title'))
put('mp3-ape-tag.mp3', AUDIO + b'APETAGEX' + bytes(24) + bytes(100))
put('mp3-lyrics3.mp3', AUDIO + b'LYRICSBEGIN' + bytes(10) + b'LYRICSEND' + v1tag(b'x'))

# ID3v2.
for major in (3, 4):
    p = 'mp3-v2%d' % major
    T = lambda n, v, e=0, f=0: text(major, n, v, e, f)
    put(p + '-basic.mp3', v2tag(major, [T(b'TIT2', 'Song'), T(b'TPE1', 'Artist'), T(b'TALB', 'Album'), T(b'TYER' if major == 3 else b'TDRC', '1999'), T(b'TRCK', '3/12'), T(b'TCON', 'Rock')], pad=64) + AUDIO)
    put(p + '-padding0.mp3', v2tag(major, [T(b'TIT2', 'Song')]) + AUDIO)
    put(p + '-padding-junk.mp3', v2tag(major, [T(b'TIT2', 'Song'), b'\x01' * 20], pad=0) + AUDIO)
    put(p + '-and-v1.mp3', v2tag(major, [T(b'TIT2', 'Song')], pad=20) + AUDIO + v1tag(b'Old'))
    put(p + '-empty-with-v1.mp3', v2tag(major, [], pad=20) + AUDIO + v1tag(b'Old', b'Artist'))
    put(p + '-with-info.mp3', v2tag(major, [T(b'TIT2', 'Song')], pad=40) + info_frame(b'Info', frames=12, size=len(body) + 417, lame=lame_tag(b'LAME3.99r', 0, 1, 19500, flags=2)) + body)
    put(p + '-genres.mp3', v2tag(major, [T(b'TCON', '(13)Pop'), T(b'TIT2', 'x')], pad=8) + AUDIO)
    for i, g in enumerate(['(17)', '13', '(13)(17)Refined', 'Pop/Funk', 'Rock;Pop', 'Rock/Pop;Jazz', '((Escaped)', '(255)', '(999)', '999', 'Unknown', '0', '(0)', 'Blues\0Rock', 'Rock\0Rock', ' Pop ', '(RX)', 'RX', 'Funk / Soul']):
        put(p + '-genre-%02d.mp3' % i, v2tag(major, [T(b'TCON', g)], pad=8) + AUDIO)
    for i, t in enumerate(['3', '3/12', '/12', '0', '3/', 'a/b/c', ' 5 / 9 ']):
        put(p + '-track-%d.mp3' % i, v2tag(major, [T(b'TRCK', t)], pad=8) + AUDIO)
    if major == 4:
        put(p + '-year-recording.mp3', v2tag(4, [text(4, b'TDRC', '2004-05-06')], pad=8) + AUDIO)
        put(p + '-year-both.mp3', v2tag(4, [text(4, b'TDRC', '2004-05-06'), text(4, b'TYER', '1999')], pad=8) + AUDIO)
    put(p + '-multi-text.mp3', v2tag(major, [T(b'TPE1', 'A\0B', 0), T(b'TPE1', 'C')], pad=8) + AUDIO)
    put(p + '-text-zero.mp3', v2tag(major, [T(b'TIT2', '0'), T(b'TPE1', 'A\x000')], pad=8) + AUDIO)
    put(p + '-text-trailing-nul.mp3', v2tag(major, [v2frame(major, b'TIT2', b'\0Song\0')], pad=8) + AUDIO)
    put(p + '-text-spaces.mp3', v2tag(major, [T(b'TIT2', '  Song  ')], pad=8) + AUDIO)
    put(p + '-text-empty.mp3', v2tag(major, [v2frame(major, b'TIT2', b'\0'), T(b'TPE1', 'x')], pad=8) + AUDIO)
    put(p + '-frame-empty.mp3', v2tag(major, [v2frame(major, b'TIT2', b''), T(b'TPE1', 'x')], pad=8) + AUDIO)
    put(p + '-latin1.mp3', v2tag(major, [T(b'TIT2', 'Caf\xe9 \xfc')], pad=8) + AUDIO)
    put(p + '-utf16.mp3', v2tag(major, [T(b'TIT2', 'Caf\xe9 ☃', 1), T(b'TPE1', 'Bj\xf6rk', 1)], pad=8) + AUDIO)
    put(p + '-utf16-emoji.mp3', v2tag(major, [T(b'TIT2', 'a\U0001f600b', 1)], pad=8) + AUDIO)
    put(p + '-utf16-odd.mp3', v2tag(major, [v2frame(major, b'TIT2', b'\x01\xff\xfeA\0B')], pad=8) + AUDIO)
    put(p + '-utf16-nobom.mp3', v2tag(major, [v2frame(major, b'TIT2', b'\x01A\0B\0')], pad=8) + AUDIO)
    put(p + '-utf16-bom-only.mp3', v2tag(major, [v2frame(major, b'TIT2', b'\x01\xff\xfe'), T(b'TPE1', 'x')], pad=8) + AUDIO)
    put(p + '-utf16-zero.mp3', v2tag(major, [T(b'TIT2', '0', 1)], pad=8) + AUDIO)
    put(p + '-utf16-be-bom.mp3', v2tag(major, [v2frame(major, b'TIT2', b'\x01\xfe\xff\0A\0B')], pad=8) + AUDIO)
    put(p + '-utf16-multi.mp3', v2tag(major, [T(b'TPE1', 'A\0B', 1)], pad=8) + AUDIO)
    put(p + '-utf8-in-v3.mp3', v2tag(major, [text(major, b'TIT2', 'Caf\xe9', 3)], pad=8) + AUDIO)
    put(p + '-bad-encoding.mp3', v2tag(major, [v2frame(major, b'TIT2', b'\x09Song')], pad=8) + AUDIO)
    put(p + '-markup.mp3', v2tag(major, [T(b'TIT2', '<script>alert(1)</script><b>x</b>'), T(b'TPE1', '5 < 6 & "q"')], pad=8) + AUDIO)
    put(p + '-comm.mp3', v2tag(major, [comm(major, '', 'Nice'), comm(major, 'iTunNORM', ' 000 '), comm(major, 'x', 'y')], pad=8) + AUDIO)
    put(p + '-comm-dup.mp3', v2tag(major, [comm(major, 'd', 'one'), comm(major, 'd', 'two'), comm(major, '', 'a'), comm(major, '', 'b')], pad=8) + AUDIO)
    put(p + '-comm-utf16.mp3', v2tag(major, [comm(major, 'd', 'Caf\xe9', 1), comm(major, '', 'Bj\xf6rk', 1)], pad=8) + AUDIO)
    put(p + '-comm-short.mp3', v2tag(major, [v2frame(major, b'COMM', b'\0en'), T(b'TIT2', 'x')], pad=8) + AUDIO)
    put(p + '-comm-noterm.mp3', v2tag(major, [v2frame(major, b'COMM', b'\0engnoterminator')], pad=8) + AUDIO)
    put(p + '-txxx.mp3', v2tag(major, [txxx(major, 'REPLAYGAIN', ' -3 dB '), txxx(major, '', 'plain'), txxx(major, 'REPLAYGAIN', 'dup')], pad=8) + AUDIO)
    put(p + '-empty-values.mp3', v2tag(major, [txxx(major, 'Empty', ''), comm(major, 'd', ''), T(b'TIT2', 'Kept')], pad=8) + AUDIO)
    bad16 = b'\xff\xfe\x00\xd8A\x00'
    put(p + '-txxx-bad-encoding.mp3', v2tag(major, [v2frame(major, b'TXXX', b'\x07d\0v'), T(b'TIT2', 'x')], pad=8) + AUDIO)
    put(p + '-txxx-noterm.mp3', v2tag(major, [v2frame(major, b'TXXX', b'\0no terminator'), T(b'TIT2', 'x')], pad=8) + AUDIO)
    put(p + '-txxx-bad-desc.mp3', v2tag(major, [v2frame(major, b'TXXX', b'\x01' + bad16 + b'\0\0' + b'\xff\xfev\x00'), T(b'TIT2', 'x')], pad=8) + AUDIO)
    put(p + '-txxx-bad-value.mp3', v2tag(major, [v2frame(major, b'TXXX', b'\x01\xff\xfed\x00\0\0' + bad16), T(b'TIT2', 'x')], pad=8) + AUDIO)
    put(p + '-comm-bad-encoding.mp3', v2tag(major, [v2frame(major, b'COMM', b'\x07engd\0v'), T(b'TIT2', 'x')], pad=8) + AUDIO)
    put(p + '-comm-bad-value.mp3', v2tag(major, [v2frame(major, b'COMM', b'\x01eng\xff\xfed\x00\0\0' + bad16), T(b'TIT2', 'x')], pad=8) + AUDIO)
    put(p + '-apic-bad-encoding.mp3', v2tag(major, [v2frame(major, b'APIC', b'\x07image/png\0\x03d\0' + png())], pad=8) + AUDIO)
    put(p + '-apic-desc-noterm.mp3', v2tag(major, [v2frame(major, b'APIC', b'\0image/png\0\x03no terminator')], pad=8) + AUDIO)
    put(p + '-length.mp3', v2tag(major, [T(b'TLEN', '1234'), T(b'TIT2', 'x')], pad=8) + AUDIO)
    put(p + '-comm-five-bytes.mp3', v2tag(major, [v2frame(major, b'COMM', b'\0engX'), T(b'TIT2', 'x')], pad=8) + AUDIO)
    put(p + '-genre-empty-brackets.mp3', v2tag(major, [T(b'TCON', '()Rock')], pad=8) + AUDIO)
    put(p + '-utf16-nobom-zero.mp3', v2tag(major, [v2frame(major, b'TIT2', b'\x010\x00')], pad=8) + AUDIO)
    put(p + '-text-first-empty.mp3', v2tag(major, [T(b'TPE1', '\x00B')], pad=8) + AUDIO)
    put(p + '-txxx-utf16.mp3', v2tag(major, [txxx(major, 'K\xe9y', 'Caf\xe9', 1)], pad=8) + AUDIO)
    put(p + '-ignored-frames.mp3', v2tag(major, [v2frame(major, b'PRIV', b'owner\0data'), v2frame(major, b'GEOB', b'\0text/plain\0a\0b\0data'), v2frame(major, b'PCNT', b'\0\0\0\x05'), v2frame(major, b'POPM', b'me\0\x80\0\0\0\x01'), v2frame(major, b'UFID', b'own\0id'), T(b'TIT2', 'Kept')], pad=8) + AUDIO)
    put(p + '-url-frame.mp3', v2tag(major, [v2frame(major, b'WOAR', b'http://example.com/'), T(b'TIT2', 'x')], pad=8) + AUDIO)
    put(p + '-wxxx.mp3', v2tag(major, [v2frame(major, b'WXXX', b'\0desc\0http://example.com/')], pad=8) + AUDIO)
    put(p + '-uslt.mp3', v2tag(major, [v2frame(major, b'USLT', b'\0eng\0Lyrics\0words')], pad=8) + AUDIO)
    put(p + '-user.mp3', v2tag(major, [v2frame(major, b'USER', b'\0eng terms')], pad=8) + AUDIO)
    put(p + '-ipls.mp3', v2tag(major, [v2frame(major, b'IPLS', b'\0a\0b\0')], pad=8) + AUDIO)
    put(p + '-unknown-frame.mp3', v2tag(major, [v2frame(major, b'ZZZZ', b'data'), v2frame(major, b'TZZZ', b'\0text'), T(b'TIT2', 'x')], pad=8) + AUDIO)
    put(p + '-lowercase-frame.mp3', v2tag(major, [v2frame(major, b'rgad', b'data'), T(b'TIT2', 'x')], pad=8) + AUDIO)
    put(p + '-bad-frame-size.mp3', v2tag(major, [b'TIT2' + be32(5000) + b'\0\0' + b'\0Song'], pad=8) + AUDIO)
    put(p + '-bad-frame-name.mp3', v2tag(major, [b'tit2' + be32(5) + b'\0\0' + b'\0Song', T(b'TPE1', 'x')], pad=8) + AUDIO)
    put(p + '-zero-size-frame.mp3', v2tag(major, [v2frame(major, b'COMM', b''), v2frame(major, b'APIC', b''), T(b'TIT2', 'x')], pad=8) + AUDIO)
    put(p + '-unsync.mp3', v2tag(major, [T(b'TIT2', 'a\xffb', 0)], pad=8, unsync=(major == 3)) + AUDIO)
    put(p + '-frame-compressed.mp3', v2tag(major, [v2frame(major, b'TIT2', b'\0\0\0\0\x05xxxxx', 0x80 if major == 3 else 0x08)], pad=8) + AUDIO)
    put(p + '-frame-encrypted.mp3', v2tag(major, [v2frame(major, b'TIT2', b'\0Song', 0x40 if major == 3 else 0x04)], pad=8) + AUDIO)
    put(p + '-frame-grouped.mp3', v2tag(major, [v2frame(major, b'TIT2', b'\x01\0Song', 0x20 if major == 3 else 0x40)], pad=8) + AUDIO)
    put(p + '-footer-flag.mp3', v2tag(major, [T(b'TIT2', 'x')], pad=8, flags=0x10) + AUDIO)
    put(p + '-truncated-tag.mp3', (v2tag(major, [T(b'TIT2', 'x')], pad=8) + AUDIO)[:20])
    put(p + '-huge-size.mp3', b'ID3' + bytes([major, 0, 0]) + synchsafe(0x0fffffff) + AUDIO)

# Extended headers and per frame flags.
put('mp3-v23-exthead.mp3', v2tag(3, [text(3, b'TIT2', 'Song')], pad=8, flags=0x40, ext=struct.pack('>IHI', 6, 0, 0) + b'') + AUDIO)
put('mp3-v23-exthead-crc.mp3', v2tag(3, [text(3, b'TIT2', 'Song')], pad=8, flags=0x40, ext=struct.pack('>IHII', 10, 0x8000, 0, 12345)) + AUDIO)
put('mp3-v23-exthead-padding.mp3', v2tag(3, [text(3, b'TIT2', 'Song')], pad=8, flags=0x40, ext=struct.pack('>IHI', 6, 0, 4) + bytes(4)) + AUDIO)
put('mp3-v24-exthead.mp3', v2tag(4, [text(4, b'TIT2', 'Song')], pad=8, flags=0x40, ext=synchsafe(6) + b'\x01\x00') + AUDIO)
put('mp3-v24-exthead-crc.mp3', v2tag(4, [text(4, b'TIT2', 'Song')], pad=8, flags=0x40, ext=synchsafe(12) + b'\x01\x20\x05' + bytes(5)) + AUDIO)
put('mp3-v24-exthead-update.mp3', v2tag(4, [text(4, b'TIT2', 'Song')], pad=8, flags=0x40, ext=synchsafe(8) + b'\x01\x40\x00') + AUDIO)
put('mp3-v24-exthead-restrict.mp3', v2tag(4, [text(4, b'TIT2', 'Song')], pad=8, flags=0x40, ext=synchsafe(8) + b'\x01\x10\x01\x00') + AUDIO)
put('mp3-v24-exthead-all.mp3', v2tag(4, [text(4, b'TIT2', 'Song')], pad=8, flags=0x40, ext=synchsafe(16) + b'\x01\x70\x00\x05' + bytes(5) + b'\x01\x00') + AUDIO)
put('mp3-v24-exthead-four-flags.mp3', v2tag(4, [text(4, b'TIT2', 'Song')], pad=8, flags=0x40, ext=synchsafe(9) + b'\x04' + bytes(4)) + AUDIO)
put('mp3-v24-exthead-five-flags.mp3', v2tag(4, [text(4, b'TIT2', 'Song')], pad=8, flags=0x40, ext=synchsafe(10) + b'\x05' + bytes(5)) + AUDIO)
put('mp3-v23-apic-gif-no-width.mp3', v2tag(3, [apic(3, b'GIF89a\x00\x00\x02\x00\x80\x00\x00' + gif()[13:], b'image/gif')], pad=8) + AUDIO)
put('mp3-v24-exthead-short.mp3', v2tag(4, [], pad=0, flags=0x40, ext=b'\0\0') + AUDIO)
put('mp3-v23-exthead-short.mp3', v2tag(3, [], pad=0, flags=0x40, ext=b'\0\0') + AUDIO)
put('mp3-v24-frame-unsync.mp3', v2tag(4, [v2frame(4, b'TIT2', b'\0a\xff\x00b', 0x02)], pad=8) + AUDIO)
put('mp3-v24-frame-dli.mp3', v2tag(4, [v2frame(4, b'TIT2', synchsafe(5) + b'\0Song', 0x01)], pad=8) + AUDIO)
put('mp3-v24-sync-unsafe-sizes.mp3', v2tag(3, [text(3, b'TIT2', 'x' * 200), text(3, b'TPE1', 'y')], pad=8).replace(b'ID3\x03', b'ID3\x04', 1) + AUDIO)
put('mp3-v24-sync-unsafe-last.mp3', v2tag(3, [text(3, b'TIT2', 'x' * 200)], pad=8).replace(b'ID3\x03', b'ID3\x04', 1) + AUDIO)
put('mp3-v22.mp3', b'ID3\x02\x00\x00' + synchsafe(16) + b'TT2\x00\x00\x05\x00Song' + bytes(5) + AUDIO)
put('mp3-v25.mp3', b'ID3\x05\x00\x00' + synchsafe(16) + bytes(16) + AUDIO)
put('mp3-v2-then-garbage.mp3', v2tag(3, [text(3, b'TIT2', 'x')], pad=8) + bytes(30) + AUDIO)
put('mp3-v2-no-frames.mp3', b'ID3\x03\x00\x00' + synchsafe(0) + AUDIO)
put('mp3-v2-nonascii-name.mp3', v2tag(3, [v2frame(3, b'T\xc9T2', b'\0Song'), text(3, b'TPE1', 'x')], pad=8) + AUDIO)

# Pictures.
for name, image, mime in (('png', png(), b'image/png'), ('gif', gif(), b'image/gif'), ('jpeg', jpeg(), b'image/jpeg')):
    for major in (3, 4):
        put('mp3-v2%d-apic-%s.mp3' % (major, name), v2tag(major, [text(major, b'TIT2', 'Cover'), apic(major, image, mime)], pad=8) + AUDIO)
put('mp3-v23-apic-wrong-mime.mp3', v2tag(3, [apic(3, png(), b'image/jpeg')], pad=8) + AUDIO)
put('mp3-v23-apic-desc.mp3', v2tag(3, [apic(3, png(), b'image/png', 4, b'front cover')], pad=8) + AUDIO)
put('mp3-v24-apic-utf16-desc.mp3', v2tag(4, [v2frame(4, b'APIC', b'\x01image/png\0\x03\xff\xfe' + 'Caf\xe9'.encode('utf-16-le') + b'\0\0' + png())], pad=8) + AUDIO)
put('mp3-v23-apic-two.mp3', v2tag(3, [apic(3, png(), b'image/png'), apic(3, gif(), b'image/gif', 4)], pad=8) + AUDIO)
put('mp3-v23-apic-bad-then-good.mp3', v2tag(3, [apic(3, b'not an image'), apic(3, png(), b'image/png')], pad=8) + AUDIO)
put('mp3-v23-apic-not-image.mp3', v2tag(3, [apic(3, b'not an image at all')], pad=8) + AUDIO)
put('mp3-v23-apic-empty-data.mp3', v2tag(3, [apic(3, b'')], pad=8) + AUDIO)
put('mp3-v23-apic-zero-data.mp3', v2tag(3, [apic(3, b'0')], pad=8) + AUDIO)
put('mp3-v23-apic-webp.mp3', v2tag(3, [apic(3, b'RIFF\x1a\0\0\0WEBPVP8L\x0d\0\0\0/\0\0\0\x10\x07\x10\x11\x11\x88\x88\xfe\x07\0', b'image/webp')], pad=8) + AUDIO)
put('mp3-v23-apic-truncated.mp3', v2tag(3, [v2frame(3, b'APIC', b'\0image/png\0\x03')], pad=8) + AUDIO)
put('mp3-v23-apic-nomime-term.mp3', v2tag(3, [v2frame(3, b'APIC', b'\0image/png')], pad=8) + AUDIO)
put('mp3-v23-apic-utf16.mp3', v2tag(3, [v2frame(3, b'APIC', b'\x01image/png\0\x03' + b'\xff\xfe' + 'd'.encode('utf-16-le') + b'\0\0' + png())], pad=8) + AUDIO)
put('mp3-v23-apic-big.mp3', v2tag(3, [apic(3, png(40, 30), b'image/png')], pad=8) + AUDIO)
put('mp3-v23-apic-and-v1.mp3', v2tag(3, [apic(3, png(), b'image/png')], pad=8) + AUDIO + v1tag(b'Old'))
