# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The audio and video files reference/getid3/golden.sh asks getID3 about:
# usage `samples.py <corpus> <out>`. The corpus's own audio and video, and
# synthetic files of the shapes the readers take and the shapes they leave:
# WAVE files of many formats and chunk layouts. Deterministic.
import os
import struct
import sys

corpus, out = sys.argv[1], sys.argv[2]
os.makedirs(out, exist_ok=True)
MEDIA = ('.mp3', '.wav', '.flac', '.ogg', '.m4a', '.aac', '.wma', '.mid', '.mp4', '.mov', '.avi', '.webm', '.mkv', '.3gp', '.wmv', '.flv', '.mpg')


def put(name, data):
    with open(os.path.join(out, name), 'wb') as f:
        f.write(data)


for name in sorted(os.listdir(corpus)):
    if name.endswith(MEDIA):
        put('corpus-' + name, open(os.path.join(corpus, name), 'rb').read())


def chunk(kind, body):
    return kind + struct.pack('<I', len(body)) + body + (b'\0' if len(body) & 1 else b'')


def fmt(tag=1, channels=1, rate=22050, bits=16, avg=None, extra=b''):
    avg = channels * rate * bits // 8 if avg is None else avg
    return chunk(b'fmt ', struct.pack('<HHIIHH', tag, channels, rate, avg, channels * bits // 8, bits) + extra)


def wave(*chunks, size=None):
    body = b'WAVE' + b''.join(chunks)
    return b'RIFF' + struct.pack('<I', len(body) if size is None else size) + body


def samples(n, width=2):
    return bytes((i * 7 + 3) & 255 for i in range(n * width))


put('wav-mono16.wav', wave(fmt(), chunk(b'data', samples(11025))))
put('wav-stereo16.wav', wave(fmt(channels=2, rate=44100), chunk(b'data', samples(8820, 4))))
put('wav-mono8.wav', wave(fmt(rate=8000, bits=8), chunk(b'data', samples(4000, 1))))
put('wav-mono24.wav', wave(fmt(bits=24), chunk(b'data', samples(2000, 3))))
put('wav-stereo8-odd.wav', wave(fmt(channels=2, rate=11025, bits=8), chunk(b'data', samples(600, 1) + b'\x01')))
put('wav-empty.wav', wave(fmt(), chunk(b'data', b''), chunk(b'JUNK', b'\0' * 600)))
put('wav-odd-data.wav', wave(fmt(bits=8), chunk(b'data', samples(601, 1))))
put('wav-fmt18.wav', wave(fmt(extra=b'\0\0'), chunk(b'data', samples(1000))))
put('wav-list-before.wav', wave(fmt(), chunk(b'LIST', b'INFOISFT\x05\0\0\0Lavf\0'), chunk(b'data', samples(1000))))
put('wav-list-after.wav', wave(fmt(), chunk(b'data', samples(1000)), chunk(b'LIST', b'INFOISFT\x05\0\0\0Lavf\0')))
put('wav-fact.wav', wave(fmt(), chunk(b'fact', struct.pack('<I', 1000)), chunk(b'data', samples(1000))))
put('wav-junk.wav', wave(chunk(b'JUNK', b'\0' * 28), fmt(), chunk(b'data', samples(1000))))
put('wav-data-first.wav', wave(chunk(b'data', samples(1000)), fmt()))
put('wav-average.wav', wave(fmt(avg=12345), chunk(b'data', samples(1000))))
put('wav-average-zero.wav', wave(fmt(avg=0), chunk(b'data', samples(1000))))
put('wav-rate-high.wav', wave(fmt(rate=192000), chunk(b'data', samples(1920))))
put('wav-long.wav', wave(fmt(rate=10, bits=8), chunk(b'data', samples(10 * 3725, 1))))
put('wav-float.wav', wave(fmt(tag=3, bits=32), chunk(b'data', samples(1000, 4))))
put('wav-three.wav', wave(fmt(channels=3), chunk(b'data', samples(1000, 6))))
put('wav-alaw.wav', wave(fmt(tag=6, bits=8), chunk(b'data', samples(1000, 1))))
put('wav-extensible.wav', wave(fmt(tag=0xFFFE, extra=b'\x16\0' + b'\0' * 22), chunk(b'data', samples(1000))))
put('wav-bits-zero.wav', wave(fmt(bits=0), chunk(b'data', samples(1000))))
put('wav-no-data.wav', wave(fmt(), chunk(b'JUNK', b'\0' * 600)))
put('wav-no-fmt.wav', wave(chunk(b'data', samples(1000))))
put('wav-two-fmt.wav', wave(fmt(), fmt(), chunk(b'data', samples(1000))))
put('wav-size-wrong.wav', wave(fmt(), chunk(b'data', samples(1000)), size=100))
put('wav-trailing.wav', wave(fmt(), chunk(b'data', samples(1000))) + b'garbage')
put('wav-cut.wav', wave(fmt(), chunk(b'data', samples(1000)))[:-100])
put('wav-other-chunk.wav', wave(fmt(), chunk(b'cue ', b'\0' * 4), chunk(b'data', samples(1000))))
put('wav-bext.wav', wave(fmt(), chunk(b'bext', b'\0' * 602), chunk(b'data', samples(1000))))
put('wav-id3.wav', b'ID3\x03\0\0\0\0\0\0' + wave(fmt(), chunk(b'data', samples(1000))))
put('wav-header-only.wav', b'RIFF\x04\0\0\0WAVE')
put('wav-form-avi.wav', b'RIFF' + struct.pack('<I', 604) + b'AVI ' + b'\0' * 600)
put('wav-form-short.wav', b'RIFF\x04\0\0\0WAV')
put('wav-riff-short.wav', b'RIFF')
put('wav-rate-zero.wav', wave(fmt(rate=0, avg=100), chunk(b'data', samples(1000))))
put('wav-channels-zero.wav', wave(fmt(channels=0, avg=100), chunk(b'data', samples(1000))))
put('wav-fmt-short.wav', wave(chunk(b'fmt ', b'\1\0\1\0\x22\x56\0\0\0\0'), chunk(b'data', samples(1000))))
put('wav-two-data.wav', wave(fmt(), chunk(b'data', samples(500)), chunk(b'data', samples(500))))
put('wav-chunk-long.wav', wave(fmt(), b'data' + struct.pack('<I', 9000) + samples(1000)))
put('wav-odd-unpadded.wav', wave(fmt(bits=8), b'data' + struct.pack('<I', 601) + samples(601, 1)))
put('wav-stray.wav', wave(fmt(), chunk(b'data', samples(1000)), b'xyz'))
put('wav-stray-long.wav', wave(fmt(), chunk(b'data', samples(1000)), b'xyzwabcdefgh'))
put('wav-pad.wav', wave(fmt(), chunk(b'PAD ', b'\0' * 4), chunk(b'data', samples(1000))))
put('wav-id3v1.wav', wave(fmt(), chunk(b'data', samples(1000))) + b'TAG' + b'Title'.ljust(30, b'\0') + b'\0' * 95)

# Formats whose module WordPress left out of its getID3: the size and type.
pad = b'\0' * 100
midi = b'MThd\0\0\0\6\0\1\0\1\0\x60MTrk\0\0\0\4\0\xff\x2f\0'
put('midi-plain.mid', midi)
put('midi-header-only.mid', b'MThd')
put('midi-id3v1.mid', midi + b'TAG' + b'Title'.ljust(30, b'\0') + b'\0' * 95)
put('midi-ape.mid', midi + pad + b'APETAGEX' + b'\0' * 24)
put('midi-lyrics.mid', midi + b'LYRICSBEGIN' + b'x' * 20 + b'LYRICSEND' + b'0' * 6 + b'LYRICS200')
put('midi-id3v2.mid', b'ID3\3\0\0\0\0\0\0' + midi)
for name, head in [('adts', b'\xff\xf1\x50\x80'), ('adts-mpeg2', b'\xff\xf9\x50\x80'), ('adts-f0', b'\xff\xf0\x50\x80'), ('adts-f8', b'\xff\xf8\x50\x80'), ('adif', b'ADIF\0\0\0\0')]:
    put('aac-%s.aac' % name, head + pad)
put('aac-id3v1.aac', b'\xff\xf1\x50\x80' + pad + b'TAG' + b'\0' * 125)
put('aac-id3v2.aac', b'ID3\3\0\0\0\0\0\0\xff\xf1\x50\x80' + pad)
for name, head in [('ps', b'\0\0\1\xba\x44'), ('seq', b'\0\0\1\xb3\x14\0'), ('system', b'\0\0\1\xbb'), ('pes', b'\0\0\1\xe0')]:
    put('mpeg-%s.mpg' % name, head + pad)
put('mpeg-ape.mpg', b'\0\0\1\xba\x44' + pad + b'APETAGEX' + b'\0' * 24)

# Small files: getID3 stops on one too small to hold the tags it looks for.
for size in (60, 100, 120, 128, 140, 144, 150, 160, 170, 200, 250, 300, 400, 600):
    put('small-%03d.wav' % size, wave(fmt(bits=8), chunk(b'data', samples(size - 44, 1))))
    put('small-%03d.mid' % size, (midi + b'\0' * size)[:size])
    put('small-%03d.aac' % size, (b'\xff\xf1\x50\x80' + b'\0' * size)[:size])
    put('small-%03d.mpg' % size, (b'\0\0\1\xba\x44' + b'\0' * size)[:size])
