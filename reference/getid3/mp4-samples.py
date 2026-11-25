# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The MP4 and QuickTime files reference/getid3/golden.sh asks getID3 about:
# usage `mp4-samples.py <out>`. Hand-built atoms in every shape the reader
# takes or leaves: video, audio and both, the movie before or after its
# data, the sample descriptions of MP4 and QuickTime, edit and time tables,
# tags and cover pictures, 64 bit and zero sizes, cut, nested and hostile
# files. The samples are headers over filler, not recordings. Deterministic.
import os
import struct
import sys
import zlib

out = sys.argv[1]
os.makedirs(out, exist_ok=True)


def put(name, data):
    with open(os.path.join(out, name), 'wb') as f:
        f.write(data)


def be16(n):
    return struct.pack('>H', n)


def be32(n):
    return struct.pack('>I', n)


def atom(name, *parts):
    body = b''.join(parts)
    return be32(8 + len(body)) + name + body


def full(version=0, flags=0):
    return bytes([version]) + flags.to_bytes(3, 'big')


def fixed(value):
    return be32(int(round(value * 65536)) & 0xffffffff)


MATRICES = {
    0: (0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000),
    90: (0, 0x10000, 0, 0xffff0000, 0, 0, 0, 0, 0x40000000),
    180: (0xffff0000, 0, 0, 0, 0xffff0000, 0, 0, 0, 0x40000000),
    270: (0, 0xffff0000, 0, 0x10000, 0, 0, 0, 0, 0x40000000),
}


def matrix(rotation=0):
    return b''.join(be32(n) for n in MATRICES[rotation])


def ftyp(major=b'isom', compatible=(b'isom', b'iso2', b'avc1', b'mp41')):
    return atom(b'ftyp', major, be32(512), *compatible)


def mvhd(scale=1000, duration=2000, created=0, version=0, extra=b''):
    return atom(b'mvhd', full(version), be32(created), be32(created), be32(scale), be32(duration),
                be32(0x10000), be16(0x100), bytes(10), matrix(), bytes(24), be32(3), extra)


def tkhd(track, duration, width=0.0, height=0.0, flags=3, rotation=0, created=0):
    return atom(b'tkhd', full(0, flags), be32(created), be32(created), be32(track), be32(0), be32(duration),
                bytes(8), be16(0), be16(0), be16(0x100 if width == 0 else 0), be16(0), matrix(rotation),
                fixed(width), fixed(height))


def mdhd(scale, duration, created=0):
    return atom(b'mdhd', full(), be32(created), be32(created), be32(scale), be32(duration), be16(0x55c4), be16(0))


def hdlr(kind, name=b'Handler', subtype=None, maker=b'\0\0\0\0'):
    return atom(b'hdlr', full(), be32(0), subtype or kind, maker, bytes(8), name + b'\0')


def dinf():
    return atom(b'dinf', atom(b'dref', full(), be32(1), atom(b'url ', full(0, 1))))


def visual(fourcc=b'avc1', width=160, height=120, vendor=b'\0\0\0\0', name=b'', depth=24, children=None):
    if children is None:
        children = atom(b'avcC', b'\x01\x64\x00\x0d\xff\xe1\x00\x04\x67\x64\x00\x0d\x01\x00\x04\x68\xeb\xe3\xcb')
    compressor = (bytes([len(name)]) + name).ljust(32, b'\0')[:32]
    body = (bytes(6) + be16(1) + be16(0) + be16(0) + vendor + be32(0) + be32(0) + be16(width) + be16(height)
            + be32(0x480000) + be32(0x480000) + be32(0) + be16(1) + compressor + be16(depth) + be16(0xffff) + children)
    return be32(8 + len(body)) + fourcc + body


def esds(avg=64000, flags=0, tags=(3, 4, 5, 6), extra=b''):
    dsi = bytes([tags[2], 2, 0x14, 0x08])
    dec = bytes([tags[1], 13 + len(dsi)]) + bytes([0x40, 0x15]) + b'\0\x01\x00' + be32(avg + 1000) + be32(avg) + dsi
    es = be16(1) + bytes([flags]) + extra + dec + bytes([tags[3], 1, 2])
    return atom(b'esds', full(), bytes([tags[0], len(es)]), es)


def sound(fourcc=b'mp4a', channels=1, bits=16, rate=22050, vendor=b'\0\0\0\0', children=None, version=0):
    if children is None:
        children = esds()
    body = (bytes(6) + be16(1) + be16(version) + be16(0) + vendor + be16(channels) + be16(bits) + be16(0) + be16(0)
            + fixed(rate) + children)
    return be32(8 + len(body)) + fourcc + body


def stsd(*entries, count=None):
    return atom(b'stsd', full(), be32(len(entries) if count is None else count), *entries)


def stts(*entries, count=None):
    return atom(b'stts', full(), be32(len(entries) if count is None else count),
                *(be32(n) + be32(d) for n, d in entries))


def tables(samples):
    return (atom(b'stsc', full(), be32(1), be32(1), be32(samples), be32(1)) +
            atom(b'stsz', full(), be32(0), be32(samples), bytes(4 * samples)) +
            atom(b'stco', full(), be32(1), be32(0)))


def trak(track, header, scale, duration, kind, entry, times, samples, extra=b'', media=None):
    head = atom(b'vmhd', full(0, 1), bytes(8)) if kind == b'vide' else atom(b'smhd', full(), bytes(4))
    stbl = atom(b'stbl', entry if entry[4:8] == b'stsd' else stsd(entry), times, tables(samples))
    return atom(b'trak', header, extra, atom(b'mdia', media if media is not None else mdhd(scale, duration),
                                             hdlr(kind), atom(b'minf', head, dinf(), stbl)))


def video(track=1, width=160, height=120, entry=None, times=None, header=None, scale=12800, duration=25600, extra=b'',
          media=None):
    return trak(track, header if header is not None else tkhd(track, 2000, width, height),
                scale, duration, b'vide', entry if entry is not None else visual(width=width, height=height),
                times if times is not None else stts((50, 512)), 50, extra, media)


def audio(track=2, entry=None, times=None, header=None, scale=22050, duration=44100, media=None):
    return trak(track, header if header is not None else tkhd(track, 2000), scale, duration, b'soun',
                entry if entry is not None else sound(), times if times is not None else stts((43, 1024), (1, 68)),
                44, b'', media)


def moov(*parts, head=None):
    return atom(b'moov', head if head is not None else mvhd(), *parts)


# Media data that no reading takes for chapter titles or an MPEG frame.
PAYLOAD = (b'\0\0\0\x10' + bytes(range(16, 32)) * 60)[:900]


def mdat(payload=PAYLOAD):
    return atom(b'mdat', payload)


def movie(*tracks, brand=None, first=False, head=None, data=None, extra=b''):
    top = ftyp() if brand is None else brand
    m = moov(*tracks, head=head) + extra
    d = mdat() if data is None else data
    return top + (m + d if first else d + m)


def png(w=2, h=2):
    def chunk(kind, body):
        return be32(len(body)) + kind + body + be32(zlib.crc32(kind + body) & 0xffffffff)
    raw = b''.join(b'\0' + b'\x80\x40\x20' * w for _ in range(h))
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b'')


def gif():
    return b'GIF89a\x02\x00\x02\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x02\x00\x02\x00\x00\x02\x02D\x01\x00;'


def jpeg():
    return (b'\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00'
            b'\xff\xdb\x00\x43\x00' + bytes([8] * 64) +
            b'\xff\xc0\x00\x0b\x08\x00\x05\x00\x03\x01\x01\x11\x00'
            b'\xff\xc4\x00\x14\x00\x01' + bytes(15) + b'\x00' +
            b'\xff\xda\x00\x08\x01\x01\x00\x00\x3f\x00\x7f\xff\xd9')


def data_box(value, flags=1):
    return atom(b'data', full(0, flags), be32(0), value)


def item(name, *boxes):
    return atom(name, *boxes)


def ilst(*items):
    return atom(b'udta', atom(b'meta', full(), hdlr(b'mdir', b'', b'mdir', b'appl'), atom(b'ilst', *items)))


V = video()
A = audio()
QT = ftyp(b'qt  ', (b'qt  ',))
MOV_VIDEO = video(entry=visual(vendor=b'FFMP', name=b'Lavc libx264'))

# Plain files: video, audio and both, the movie after or before its data.
put('mp4-video.mp4', movie(V))
put('mp4-video-first.mp4', movie(V, first=True))
put('mp4-both.mp4', movie(V, A))
put('mp4-both-first.mp4', movie(V, A, first=True))
put('mp4-audio-video.mp4', movie(audio(1), video(2)))
put('mp4-audio.m4a', movie(audio(1), brand=ftyp(b'M4A ', (b'M4A ', b'mp42', b'isom'))))
put('mp4-audio-first.m4a', movie(audio(1), first=True))
put('mp4-both.m4a', movie(V, A))
put('mp4-video.m4a', movie(V))
put('mp4-audio.mp4', movie(audio(1)))
put('mp4-both.m4v', movie(V, A, brand=ftyp(b'M4V ', (b'M4V ', b'mp42', b'isom'))))
put('mp4-both.3gp', movie(V, A, brand=ftyp(b'3gp4', (b'3gp4', b'isom'))))
put('mp4-video.3g2', movie(V, brand=ftyp(b'3g2a', (b'3g2a',))))
put('mp4-two-videos.mp4', movie(V, video(2, 320, 240)))
put('mp4-two-videos-small-last.mp4', movie(video(1, 320, 240), video(2, 160, 120)))
put('mp4-two-audios.mp4', movie(V, A, audio(3, sound(channels=2, rate=44100))))
put('mp4-two-audios.m4a', movie(audio(1), audio(2, sound(channels=2, rate=44100, children=esds(128000)))))
put('mp4-no-ftyp.mp4', moov(V, A) + mdat())
put('mp4-free-first.mp4', atom(b'free', bytes(8)) + movie(V))
put('mp4-wide-mdat.mp4', ftyp() + atom(b'wide') + mdat() + moov(V, A))
put('mp4-mdat-first.mp4', mdat() + moov(V))
put('mp4-skip-after.mp4', movie(V, A) + atom(b'skip', bytes(20)))
put('mp4-free-after-moov.mp4', movie(V, A, first=True, extra=atom(b'free')))
put('mp4-pnot.mp4', ftyp() + atom(b'pnot', be32(0), be16(0), b'PICT', be16(1)) + mdat() + moov(V))
put('mp4-unknown-top.mp4', movie(V) + atom(b'junk', b'anything at all'))
put('mp4-moov-only.mp4', ftyp() + moov(V) + atom(b'free', bytes(600)))
put('mp4-no-moov.mp4', ftyp() + mdat())
put('mp4-no-tracks.mp4', ftyp() + mdat() + moov())
put('mp4-two-mdat.mp4', ftyp() + mdat() + moov(V, A) + mdat(PAYLOAD[:300]))
put('mp4-empty-mdat-last.mp4', ftyp() + mdat() + moov(V) + atom(b'mdat'))
put('mp4-two-moov.mp4', ftyp() + mdat() + moov(V) + moov(video(1, 320, 240), head=mvhd(1000, 3000, 3786825600)))

# QuickTime: the sample description of a vendor, its compressor and depth.
put('mov-video.mov', movie(MOV_VIDEO, brand=QT))
put('mov-both.mov', movie(MOV_VIDEO, A, brand=QT))
put('mov-both-first.mov', movie(MOV_VIDEO, A, brand=QT, first=True))
put('mov-no-name.mov', movie(video(entry=visual(vendor=b'FFMP')), brand=QT))
put('mov-null-name.mov', movie(video(entry=visual(vendor=b'appl', name=b'H.264\0')), brand=QT))
put('mov-plain-name.mov', movie(video(entry=visual(vendor=b'appl', name=b'x' * 31)), brand=QT))
for depth in (1, 8, 16, 32, 33, 40, 0, 12):
    put('mov-depth-%d.mov' % depth, movie(video(entry=visual(vendor=b'FFMP', name=b'Codec', depth=depth)), brand=QT))
put('mov-twos.mov', movie(MOV_VIDEO, audio(entry=sound(b'twos', 2, 16, 44100, children=b'')), brand=QT))
put('mov-sowt.mov', movie(MOV_VIDEO, audio(entry=sound(b'sowt', 1, 16, 22050, children=b'')), brand=QT))
put('mov-lpcm.mov', movie(MOV_VIDEO, audio(entry=sound(b'lpcm', 1, 16, 22050, children=b'')), brand=QT))
put('mov-audio-vendor.mov', movie(MOV_VIDEO, audio(entry=sound(vendor=b'FFMP')), brand=QT))
put('mov-sound-v1.mov', movie(MOV_VIDEO, audio(entry=sound(version=1, children=bytes(16) + esds())), brand=QT))
put('mov-qtvr.mov', movie(video(entry=visual(b'qtvr')), brand=QT))
put('mov-hdlr-pano.mov', movie(video(media=atom(b'mdia', mdhd(600, 1200), hdlr(b'pano', subtype=b'STpn', maker=b'zzzz'))), brand=QT))
put('mov-ctyp.mov', movie(MOV_VIDEO, brand=QT, head=mvhd() + atom(b'udta', atom(b'ctyp', b'qtvr'))))
put('mov-no-ftyp.mov', moov(MOV_VIDEO) + mdat())
put('mov-wide-first.mov', atom(b'wide') + mdat() + moov(MOV_VIDEO))

# Codecs of the sample description.
for fourcc in (b'hvc1', b'hev1', b'mp4v', b'jpeg', b'SVQ3', b'SVQ1', b'v210', b'2vuY', b'rle ', b'h263', b'raw ', b'cvid', b'xvid', b'divx', b'3iv2', b'mp4s', b'zzzz',
               b'dvc ', b'dvcp', b'gif ', b'kpcd', b'mjpa', b'mjpb', b'png ', b'rpza', b'smc ', b'tiff', b'yuv2'):
    put('codec-%s.mp4' % fourcc.decode().strip(), movie(video(entry=visual(fourcc))))
put('codec-mp4v-audio.mp4', movie(video(entry=visual(b'mp4v')), A))
put('codec-mp4s-vendor.mov', movie(video(entry=visual(b'mp4s', vendor=b'FFMP')), brand=QT))
put('codec-keys-vendor.mov', movie(video(entry=visual(vendor=b'keys')), brand=QT))
put('codec-no-size.mp4', movie(video(entry=visual(width=0, height=0))))
put('codec-only-width.mp4', movie(video(entry=visual(width=200, height=0))))
put('codec-alac.m4a', movie(audio(1, sound(b'alac', children=b''))))
put('codec-stereo.m4a', movie(audio(1, sound(channels=2, rate=44100))))
put('codec-six.m4a', movie(audio(1, sound(channels=6, rate=48000))))
put('codec-zero-channels.m4a', movie(audio(1, sound(channels=0))))
put('codec-no-bits.m4a', movie(audio(1, sound(bits=0))))
put('codec-odd-rate.m4a', movie(audio(1, sound(rate=22050.5))))
put('codec-two-entries.mp4', movie(video(entry=stsd(visual(), visual(b'hvc1', 320, 240)))))
put('codec-no-entries.mp4', movie(video(entry=stsd())))
put('codec-short-zero-vendor.mp4', movie(video(entry=stsd(be32(28) + b'avc1' + bytes(6) + be16(1) + bytes(12)))))
put('codec-short-entry.mp4', movie(video(entry=stsd(be32(12) + b'avc1' + bytes(4)))))
put('codec-entries-overclaim.mp4', movie(video(entry=stsd(visual(), count=3))))
put('codec-entries-huge.mp4', movie(video(entry=stsd(visual(), count=0x01000001))))
put('codec-parrot.mp4', movie(video(entry=stsd(be32(80) + b'mp4s' + bytes(6) + be16(1) + b'\0application/octet-stream;type=com.parrot.videometadata\x01' + bytes(8)))))

# The elementary stream descriptor of AAC.
put('esds-no-average.m4a', movie(audio(1, sound(children=esds(0)))))
put('esds-none.m4a', movie(audio(1, sound(children=b''))))
put('esds-none.mp4', movie(V, audio(entry=sound(children=b''))))
put('esds-url.m4a', movie(audio(1, sound(children=esds(flags=0x40)))))
put('esds-depends.m4a', movie(audio(1, sound(children=esds(96000, 0xa0, extra=b'\0\x02\0\x03')))))
put('esds-wrong-tag.m4a', movie(audio(1, sound(children=esds(tags=(4, 4, 5, 6))))))
put('esds-wrong-config.m4a', movie(audio(1, sound(children=esds(tags=(3, 5, 5, 6))))))
put('esds-wrong-specific.m4a', movie(audio(1, sound(children=esds(tags=(3, 4, 6, 6))))))
put('esds-long-length.m4a', movie(audio(1, sound(children=atom(b'esds', full(), b'\x03\x80\x80\x80\x20', be16(1), b'\0', b'\x04\x80\x80\x0d\x40\x15\0\x01\0', be32(70000), be32(64000), b'\x05\x02\x14\x08\x06\x01\x02')))))
put('esds-cut.m4a', movie(audio(1, sound(children=atom(b'esds', full(), b'\x03\x19\0\x01\0\x04\x11\x40\x15\0\x01\0\0\x01\x11\x70\0\x00\xfa')))))
put('esds-gps-inside.m4a', movie(audio(1, sound(children=esds() + atom(b'gps ', bytes(8))))))
put('esds-outside.mp4', movie(V, extra=atom(b'esds', esds()[8:])))

# The movie header: scales, durations, versions, dates, its place.
for name, scale, duration in (('exact', 1000, 3000), ('fraction', 1000, 2345), ('long', 600, 600 * 3725),
                              ('zero', 1000, 0), ('half', 1000, 1500), ('tiny', 90000, 1)):
    put('mvhd-%s.mp4' % name, movie(V, A, head=mvhd(scale, duration)))
put('mvhd-one-second.mp4', movie(V, audio(entry=sound(children=esds(1000))), head=mvhd(1000, 1000)))
put('mvhd-zero-scale.mp4', movie(V, head=mvhd(0, 1000)))
put('mvhd-v1.mp4', movie(V, head=atom(b'mvhd', full(1), struct.pack('>QQIQ', 0, 0, 1000, 2000), bytes(80))))
put('mvhd-short.mp4', movie(V, head=atom(b'mvhd', full(), be32(0), be32(0), be32(1000))))
put('mvhd-dated.mp4', movie(V, head=mvhd(created=3786825600)))
put('mvhd-last-second.mp4', movie(V, head=mvhd(created=0xffffffff)))
put('mvhd-1970.mp4', movie(V, head=mvhd(created=2082844800)))
put('mvhd-udta-first.mp4', ftyp() + mdat() + atom(b'moov', atom(b'udta', atom(b'\xa9too', be16(4), be16(0), b'Lavf')), mvhd(), V))
put('mvhd-trak-first.mp4', ftyp() + mdat() + atom(b'moov', V, mvhd()))
put('mvhd-free-first.mp4', ftyp() + mdat() + atom(b'moov', atom(b'free', bytes(4)), mvhd(), V))
put('mvhd-none.mp4', ftyp() + mdat() + atom(b'moov', V))
put('mvhd-zero-atom-first.mp4', ftyp() + mdat() + atom(b'moov', bytes(4), mvhd(created=3786825600), V))
put('mvhd-mdhd-first.mp4', ftyp() + mdat() + atom(b'moov', mdhd(1000, 2000, 3786825600), mvhd(), V))
put('mvhd-tkhd-first.mp4', ftyp() + mdat() + atom(b'moov', tkhd(1, 2000, created=3786825600), mvhd(), V))
put('mvhd-twice.mp4', ftyp() + mdat() + atom(b'moov', mvhd(1000, 5000), V, mvhd(1000, 2000)))

# Track and media headers.
put('tkhd-disabled.mp4', movie(video(header=tkhd(1, 2000, 160, 120, flags=0), entry=visual(vendor=b'FFMP', name=b'x'))))
put('tkhd-fraction.mov', movie(video(header=tkhd(1, 2000, 160.5, 120.25), entry=visual(vendor=b'FFMP', name=b'x')), brand=QT))
put('tkhd-larger.mp4', movie(video(header=tkhd(1, 2000, 640, 480))))
put('tkhd-only.mp4', movie(video(entry=visual(b'zzzz'))))
put('tkhd-none.mp4', movie(trak(1, b'', 12800, 25600, b'vide', visual(), stts((50, 512)), 50)))
put('tkhd-short.mp4', movie(video(header=atom(b'tkhd', full(0, 3), bytes(40)))))
for rotation in (90, 180, 270):
    put('tkhd-rotated-%d.mp4' % rotation, movie(video(header=tkhd(1, 2000, 160, 120, rotation=rotation)), A))
put('tkhd-audio-rotated.mp4', movie(V, audio(header=tkhd(2, 2000, rotation=90))))
put('mdhd-small-scale.mp4', movie(video(scale=600, duration=1200, times=stts((50, 24))), A))
put('mdhd-scales.mp4', movie(video(scale=25, duration=50, times=stts((50, 1))), audio(scale=500)))
put('mdhd-zero-scale.mp4', movie(video(scale=0)))
put('mdhd-short.mp4', movie(video(media=atom(b'mdhd', full(), bytes(8)))))

# Time to sample tables: the frame rate and the counts.
put('stts-many.mp4', movie(video(times=stts((10, 512), (20, 256), (20, 1024))), A))
put('stts-zero-duration.mp4', movie(video(times=stts((50, 0))), A))
put('stts-zero-last.mp4', movie(video(times=stts((49, 512), (1, 0))), A))
put('stts-empty.mp4', movie(video(times=stts()), A))
put('stts-none.mp4', movie(video(times=b''), A))
put('stts-tiny-rate.mp4', movie(video(scale=1, duration=6000, times=stts((2, 3000))), A))
put('stts-half-milli.mp4', movie(video(scale=1, duration=4000, times=stts((2, 2000))), A))
put('stts-just-over.mp4', movie(video(scale=1, duration=3998, times=stts((2, 1999))), A))
put('stts-overclaim.mp4', movie(video(times=stts((50, 512), count=4)), A))
put('stts-cut-entry.mp4', movie(video(times=atom(b'stts', full(), be32(1), be32(50), be16(2))), A))
put('stts-huge-count.mp4', movie(video(times=stts((50, 512), count=13422)), A))
put('stts-limit.mp4', movie(video(times=stts((50, 512), count=13421)), A))
put('stts-fast.mp4', movie(video(entry=visual(b'hev1'), times=stts((50, 512))), head=mvhd(1000, 100)))
put('stts-fast-audio.mp4', movie(video(entry=visual(b'hev1'), times=stts((5000, 1))), head=mvhd(1000, 1000)))
put('stts-no-video.mp4', movie(video(entry=visual(b'hev1')), A))

# Tags and cover pictures.
TOO = item(b'\xa9too', data_box(b'Lavf62.13.101'))
put('tags-udta.mp4', movie(V, A, head=mvhd() + atom(b'udta', atom(b'\xa9nam', be16(7), be16(0x15c7), b'A title'))))
put('tags-ilst.mp4', movie(V, A, head=mvhd() + ilst(item(b'\xa9nam', data_box(b'A Title')), TOO, item(b'gnre', data_box(be16(3), 0)),
                                                    item(b'trkn', data_box(bytes(2) + be16(3) + be16(12) + bytes(2), 0)))))
put('tags-ilst.m4a', movie(audio(1), head=mvhd() + ilst(item(b'\xa9nam', data_box(b'A Title')), item(b'\xa9ART', data_box(b'An Artist')), TOO)))
for name, image, flags in (('jpeg', jpeg(), 13), ('png', png(), 14), ('gif', gif(), 0), ('jpeg-text', jpeg(), 1),
                           ('png-zero', png(), 0), ('jpeg-21', jpeg(), 21), ('png-other', png(), 27)):
    put('cover-%s.m4a' % name, movie(audio(1), head=mvhd() + ilst(TOO, item(b'covr', data_box(image, flags)))))
    put('cover-%s.mp4' % name, movie(V, A, head=mvhd() + ilst(item(b'covr', data_box(image, flags)))))
put('cover-two.m4a', movie(audio(1), head=mvhd() + ilst(item(b'covr', data_box(png(), 14), data_box(jpeg(), 13)))))
put('cover-two-atoms.m4a', movie(audio(1), head=mvhd() + ilst(item(b'covr', data_box(gif(), 0)), item(b'covr', data_box(png(), 14)))))
put('cover-bmp.m4a', movie(audio(1), head=mvhd() + ilst(item(b'covr', data_box(b'BM' + bytes(60), 27)))))
put('cover-unknown.m4a', movie(audio(1), head=mvhd() + ilst(item(b'covr', data_box(b'not a picture', 13)))))
put('cover-unknown.mp4', movie(V, head=mvhd() + ilst(item(b'covr', data_box(b'not a picture', 13)))))
put('cover-unknown-zero.m4a', movie(audio(1), head=mvhd() + ilst(item(b'covr', data_box(b'not a picture', 0)))))
put('cover-unknown-then-png.m4a', movie(audio(1), head=mvhd() + ilst(item(b'covr', data_box(png(), 0), data_box(b'not a picture', 0)))))
put('cover-empty.m4a', movie(audio(1), head=mvhd() + ilst(item(b'covr', data_box(b'', 13)))))
put('cover-empty-zero.m4a', movie(audio(1), head=mvhd() + ilst(item(b'covr', data_box(b'', 0), data_box(png(), 14)))))
put('cover-zero-text.m4a', movie(audio(1), head=mvhd() + ilst(item(b'covr', data_box(b'0', 1)))))
put('cover-mean-name.m4a', movie(audio(1), head=mvhd() + ilst(item(b'covr', atom(b'mean', full(), b'com.apple.iTunes'), atom(b'name', full(), b'cover'), data_box(png(), 14)))))
put('cover-other-box.m4a', movie(audio(1), head=mvhd() + ilst(item(b'covr', atom(b'junk', b'1234'), data_box(png(), 14)))))
put('cover-box-one.m4a', movie(audio(1), head=mvhd() + ilst(item(b'covr', be32(1) + b'data' + png()))))
put('cover-box-small.m4a', movie(audio(1), head=mvhd() + ilst(item(b'covr', be32(5) + b'data' + png()))))
put('cover-box-long.m4a', movie(audio(1), head=mvhd() + ilst(item(b'covr', be32(4000) + b'data' + full(0, 14) + be32(0) + png()))))
put('cover-flags-short.m4a', movie(audio(1), head=mvhd() + ilst(item(b'covr', be32(10) + b'data' + b'\0\0'))))
put('cover-smallbox.m4a', movie(audio(1), head=mvhd() + ilst(item(b'covr', be16(len(png())) + b'\x10\xb5' + png()))))
put('cover-udta.m4a', movie(audio(1), head=mvhd() + atom(b'udta', atom(b'covr', be16(10), be16(0), png()))))
put('cover-top.m4a', movie(audio(1)) + item(b'covr', data_box(png(), 14)))
put('cover-in-udta-and-ilst.mp4', movie(V, head=mvhd() + atom(b'udta', atom(b'\xa9nam', be16(1), be16(0), b'x'), atom(b'meta', full(), atom(b'ilst', item(b'covr', data_box(jpeg(), 13)))))))
put('cover.mov', movie(MOV_VIDEO, A, brand=QT, head=mvhd() + ilst(item(b'covr', data_box(png(), 14)))))
put('cover.m4v', movie(V, A, brand=ftyp(b'M4V ', (b'M4V ',)), head=mvhd() + ilst(item(b'covr', data_box(jpeg(), 13)))))
put('tags-meta-quicktime.mov', movie(MOV_VIDEO, brand=QT, head=mvhd() + atom(b'meta', atom(b'hdlr', full(), be32(0), b'mdta', bytes(12), b'\0'))))
put('tags-meta-numeric.mov', movie(MOV_VIDEO, brand=QT, head=mvhd() + atom(b'meta', full(), atom(b'ilst', atom(b'\0\0\0\x01', data_box(b'x'))))))
put('tags-keys.mov', movie(MOV_VIDEO, brand=QT, head=mvhd() + atom(b'meta', full(), atom(b'keys', full(), be32(1), be32(12), b'mdta', b'test'))))

# Sizes: 64 bit, zero, cut, past the end, too small.
put('size-64-mdat.mp4', ftyp() + be32(1) + b'mdat' + struct.pack('>Q', 16 + len(PAYLOAD)) + PAYLOAD + moov(V, A))
put('size-64-moov.mp4', ftyp() + mdat() + be32(1) + b'moov' + struct.pack('>Q', 16 + len(moov(V)) - 8) + moov(V)[8:])
put('size-64-small.mp4', ftyp() + mdat() + moov(V) + be32(1) + b'free' + struct.pack('>Q', 8))
put('size-64-zero.mp4', ftyp() + mdat() + moov(V) + be32(1) + b'free' + struct.pack('>Q', 0))
put('size-64-huge.mp4', ftyp() + mdat() + moov(V) + be32(1) + b'free' + struct.pack('>Q', 1 << 63))
put('size-64-cut.mp4', ftyp() + mdat() + moov(V) + be32(1) + b'free' + b'\0\0\0')
put('size-zero-last.mp4', ftyp() + moov(V, A) + be32(0) + b'mdat' + PAYLOAD)
put('size-zero-mid.mp4', ftyp() + be32(0) + b'mdat' + PAYLOAD + moov(V, A))
put('size-zero-tail.mp4', movie(V, A) + bytes(4))
put('size-tail-bytes.mp4', movie(V, A) + b'\x01\x02\x03')
put('size-tail-zeros.mp4', movie(V, A) + bytes(7))
put('size-small-top.mp4', movie(V, A) + be32(7) + b'free')
put('size-two-top.mp4', ftyp() + be32(2) + b'free' + mdat() + moov(V))
whole = movie(V, A)
put('size-cut-mdat.mp4', (ftyp() + mdat() + moov(V, A))[:len(ftyp()) + 700])
put('size-cut-moov.mp4', whole[:-40])
put('size-cut-first.mp4', movie(V, A, first=True)[:-100])
put('size-cut-mpeg4.mp4', movie(video(entry=visual(b'mp4v')), A, first=True)[:-100])
put('size-past-end.mp4', ftyp() + mdat() + moov(V)[:4] + b'moox' + moov(V)[8:] + be32(0x7fffffff) + b'free')
put('size-sub-zero.mp4', ftyp() + mdat() + atom(b'moov', mvhd(), bytes(4), V))
put('size-sub-zero-end.mp4', ftyp() + mdat() + atom(b'moov', mvhd(), V, bytes(4)))
put('size-sub-tiny.mp4', ftyp() + mdat() + atom(b'moov', mvhd(), be32(4) + b'free', V))
put('size-sub-past.mp4', ftyp() + mdat() + atom(b'moov', mvhd(), V, be32(400) + b'free' + bytes(20)))
put('size-moov-four-zeros.mp4', ftyp() + mdat() + atom(b'moov', bytes(4)))
put('size-moov-eight-zeros.mp4', ftyp() + mdat() + atom(b'moov', bytes(8)))
put('size-moov-empty.mp4', ftyp() + mdat() + atom(b'moov'))

# Media data: the shapes getID3 reads as chapter titles or an MPEG stream.
put('mdat-chapters.mp4', ftyp() + mdat(be16(7) + b'Chapter' + PAYLOAD) + moov(V))
put('mdat-chapter-newline.mp4', ftyp() + mdat(be16(4) + b'abc\n' + bytes(400)) + moov(V))
put('mdat-wide-chapters.mp4', ftyp() + mdat(be32(8) + b'wide' + be32(0) + b'mdat' + be16(5) + b'Intro' + PAYLOAD) + moov(V))
put('mdat-wide-inside.mp4', ftyp() + mdat(be32(8) + b'wide' + PAYLOAD) + moov(V))
put('mdat-gpro.mp4', ftyp() + mdat(b'GPRO' + struct.pack('<I', 120) + b'HD9.01.01.60.00' + bytes(400)) + moov(V))
put('mdat-mpeg-after.mp4', ftyp() + mdat() + atom(b'free', b'\xff\xfb\x90\x64' + bytes(400)) + moov(V))
put('mdat-mpeg-inside.mp4', ftyp() + mdat(b'\xff\xfb\x90\x64' + bytes(600)) + moov(V))
put('mdat-nested.mp4', ftyp() + mdat() + moov(V, atom(b'mdat', bytes(16))))
put('mdat-eight.mp4', ftyp() + atom(b'mdat') + moov(V) + atom(b'free', PAYLOAD))
put('mdat-short-length.mp4', ftyp() + mdat(be16(1) + b'\x01' + PAYLOAD) + moov(V))

# What getID3 reads by other modules, or does not read here.
put('other-heic.mp4', ftyp(b'heic', (b'mif1', b'heic')) + mdat() + moov(V))
put('other-moof.mp4', movie(V) + atom(b'moof', atom(b'mfhd', full(), be32(1))))
put('other-cmov.mov', ftyp() + atom(b'moov', atom(b'cmov', atom(b'dcom', b'zlib'), atom(b'cmvd', be32(100), zlib.compress(bytes(100))))) + mdat())
put('other-uuid.mp4', movie(V) + atom(b'uuid', bytes(16) + b'data'))
put('other-uuid-360fly.mp4', movie(V) + atom(b'uuid', bytes.fromhex('efe1589abb7749ef809527759eb1dc6f') + bytes(64)))
put('other-gps.mp4', movie(V, extra=atom(b'gps ', be32(0) + be32(0) + be32(10) + be32(16))))
put('other-id32.mp4', movie(V, head=mvhd() + atom(b'udta', atom(b'ID32', full(), be16(0x15c7), b'ID3\x03\0\0\0\0\0\0'))))
put('other-thumbnail.mov', movie(MOV_VIDEO, brand=QT, head=mvhd() + atom(b'udta', atom(b'NCDT', atom(b'NCTH', jpeg())))))
put('other-frea.mov', movie(MOV_VIDEO, brand=QT, extra=atom(b'frea', atom(b'thma', jpeg()))))
put('other-elst.mp4', movie(video(extra=atom(b'edts', atom(b'elst', full(), be32(1), be32(2000), be32(0), be32(0x10000))))))
put('other-elst-overclaim.mp4', movie(video(extra=atom(b'edts', atom(b'elst', full(), be32(9), be32(2000), be32(0), be32(0x10000))))))
put('other-dref-overclaim.mp4', movie(video(media=atom(b'mdia', mdhd(12800, 25600), hdlr(b'vide'), atom(b'minf', atom(b'dinf', atom(b'dref', full(), be32(5), atom(b'url ', full(0, 1)))))))))
put('other-iods.mp4', movie(V, A, head=mvhd() + atom(b'iods', full(), b'\x10\x07\x00\x4f\xff\xff\x29\x15\xff')))
put('other-iods-tracks.mp4', movie(V, A, head=mvhd() + atom(b'iods', full(), b'\x10\x13\x00\x4f\xff\xff\x29\x15\xff\x0e\x04\x00\x00\x00\x01\x0e\x04\x00\x00\x00\x02')))
put('other-iods-overclaim.mp4', movie(V, A, head=mvhd() + atom(b'iods', full(), b'\x10\xff\x7f\x00\x4f\xff\xff\x29\x15\xff')))
put('other-tref.mov', movie(video(extra=atom(b'tref', atom(b'tmcd', be32(2)))), brand=QT))
put('other-ctab.mov', movie(MOV_VIDEO, brand=QT, head=mvhd() + atom(b'ctab', be32(0), be16(0x8000), be16(0), bytes(8))))
put('other-loci.3gp', movie(V, brand=ftyp(b'3gp4', (b'3gp4',)), head=mvhd() + atom(b'udta', atom(b'loci', full(), be16(0x15c7), b'Home\0', b'\0', fixed(1.5), fixed(2.5), fixed(0), b'Earth\0', b'\0'))))
put('other-chpl.m4a', movie(audio(1), head=mvhd() + atom(b'udta', atom(b'chpl', full(1), be32(0), b'\x01', struct.pack('>Q', 0), b'\x05Intro'))))
put('other-xyz.mov', movie(MOV_VIDEO, brand=QT, head=mvhd() + atom(b'udta', atom(b'\xa9xyz', be16(18), be16(0x15c7), b'+51.5074-000.1278/'))))
put('other-rcif.mov', movie(MOV_VIDEO, brand=QT, head=mvhd() + atom(b'udta', atom(b'rcif', b'\0\0\0\0\x55\xc4{"wxcamera":{"rotate":90}}'))))
put('other-mpeg-size.mp4', be32(0x1b3) + b'ftyp' + b'isom' + bytes(0x1b3 - 12) + mdat() + moov(V))
put('other-scrm.mp4', ftyp() + atom(b'free', bytes(20) + b'SCRM' + bytes(20)) + mdat() + moov(V))
put('other-mk.mp4', ftyp() + atom(b'free', bytes(1080 - 32 - 8) + b'M.K.' + bytes(8)) + mdat() + moov(V))
put('other-dss.mp4', b'\x02dss' + b'free' + bytes(600))
put('other-tagged.mp4', movie(V) + b'TAG' + bytes(125))
put('other-small.mp4', ftyp() + atom(b'mdat', bytes(8)) + atom(b'moov', mvhd()))

# Nesting and counts.
deep = atom(b'\xa9nam', be16(1), be16(0), b'x')
for level in range(40):
    deep = atom(b'udta', deep)
put('nest-40.mp4', movie(V, head=mvhd() + deep))
deep = atom(b'free')
for level in range(70):
    deep = atom(b'udta', deep)
put('nest-70.mp4', movie(V, head=mvhd() + deep))
put('count-free.mp4', movie(V, head=mvhd() + atom(b'udta', atom(b'free') * 30000)))
put('count-free-top.mp4', movie(V) + atom(b'free') * 30000)
put('count-many-tracks.mp4', movie(*(video(n + 1, 16 * (n + 1), 12 * (n + 1)) for n in range(12))))
