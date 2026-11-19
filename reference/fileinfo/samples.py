# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The files reference/fileinfo/golden.sh asks PHP's fileinfo about, beside
# the upload corpus's: usage `samples.py <corpus> <out>`. Synthetic headers
# of each family (audio, video, archives, documents), zip layouts, prose
# of many shapes, and every corpus file with bytes of its start changed.
# Deterministic: the same files each run.
import os
import random
import struct
import sys
import zipfile

corpus, out = sys.argv[1], sys.argv[2]
os.makedirs(out, exist_ok=True)
pad = b'\x00' * 200
rng = random.Random(46)


def put(name, data):
    with open(os.path.join(out, name), 'wb') as f:
        f.write(data)


# The corpus's own files, whole, and with the start changed.
kept = []
for name in sorted(os.listdir(corpus)):
    path = os.path.join(corpus, name)
    if name.endswith(('.jpg', '.png', '.gif', '.webp', '.avif', '.bmp', '.tif')) or os.path.getsize(path) > 30000:
        continue
    data = open(path, 'rb').read()
    kept.append((name, data))
    put('corpus-' + name, data)
for name, data in kept:
    if len(data) > 6000:
        continue
    for n in range(4):
        changed = bytearray(data)
        for _ in range(1 + n % 2):
            at = rng.randrange(0, min(48, len(changed)))
            changed[at] = rng.randrange(256)
        put('mutant-%d-%s' % (n, name), bytes(changed))
    for cut in (4, 12, 40):
        put('cut-%d-%s' % (cut, name), data[:cut])


def ftyp(brand):
    body = brand + b'\x00\x00\x02\x00isom'
    return struct.pack('>I', 8 + len(body)) + b'ftyp' + body + pad


for b in [b'isom', b'iso2', b'mp41', b'mp42', b'avc1', b'M4A ', b'M4B ', b'M4V ', b'M4P ', b'qt  ', b'3gp4', b'3gp5', b'3gp6', b'3g2a', b'dash', b'MSNV', b'f4v ', b'XXXX', b'crx ']:
    put('ftyp-%s.bin' % b.decode().strip().replace(' ', '_'), ftyp(b))
for atom in [b'moov', b'free', b'wide', b'mdat', b'skip']:
    put('atom-%s.bin' % atom.decode(), struct.pack('>I', 8) + atom + pad)
put('atom-small.bin', struct.pack('>I', 4) + b'moov' + pad)
for h in ['fffb9000', 'fffa9000', 'fff39000', 'fff29000', 'ffe39000', 'ffe29000', 'fffb0000', 'fffbf000', 'fffb9c00', 'fffd9000', 'fffc9000', 'fffb1000', 'fffbe000', 'fff59000', 'fff19000', 'fff99000', 'ffe19000', 'fffb9400', 'fffb9800', 'fffa9800', 'fff3e800']:
    put('mpeg-%s.bin' % h, bytes.fromhex(h) + pad)
for v in [b'\x02\x00', b'\x03\x00', b'\x04\x00', b'\x05\x00']:
    put('id3-%s-bad.bin' % v.hex(), b'ID3' + v + b'\x00\x00\x00\x00\x10' + pad)
    put('id3-%s-frame.bin' % v.hex(), b'ID3' + v + b'\x00\x00\x00\x00\x10' + b'\x00' * 16 + bytes.fromhex('fffb9000') + pad)
put('id3-footer.bin', b'ID3\x04\x00\x10\x00\x00\x00\x04' + b'\x00' * 4 + b'3DI' + b'\x00' * 7 + bytes.fromhex('fffb9000') + pad)
put('id3-short.bin', b'ID3')
put('id3-bigsize.bin', b'ID3\x03\x00\x00\xff\xff\xff\xff' + pad)
riff = lambda kind: b'RIFF' + struct.pack('<I', 200) + kind + b'fmt ' + pad
for kind in [b'WAVE', b'AVI ', b'WEBP', b'XXXX', b'RMID']:
    put('riff-%s.bin' % kind.decode().strip(), riff(kind))
put('riff-short.bin', b'RIFF\x04\x00\x00\x00WAVE')


def ogg(payload):
    return b'OggS\x00\x02' + b'\x00' * 20 + b'\x01' + bytes([len(payload)]) + payload + pad


for name, payload in [('vorbis', b'\x01vorbis' + b'\x00' * 22), ('opus', b'OpusHead\x01\x01' + b'\x00' * 10), ('theora', b'\x80theora' + b'\x00' * 22), ('flac', b'\x7fFLAC\x01\x00' + b'\x00' * 22), ('speex', b'Speex   ' + b'\x00' * 22), ('other', b'zzzzzzzz')]:
    put('ogg-%s.bin' % name, ogg(payload))
put('ogg-short.bin', b'OggS\x00\x02')
put('flac-bare.bin', b'fLaC' + pad)
put('flv.bin', b'FLV\x01\x05\x00\x00\x00\x09' + pad)
put('flv-video.bin', b'FLV\x01\x01\x00\x00\x00\x09' + pad)
put('asf.bin', bytes.fromhex('3026b2758e66cf11a6d900aa0062ce6c') + pad)
put('midi.bin', b'MThd\x00\x00\x00\x06\x00\x01\x00\x01\x00\x60' + pad)
for h in ['000001ba44', '000001b3', '000001b320', '000001bb', '000001e0']:
    put('mpegps-%s.bin' % h, bytes.fromhex(h) + pad)
put('ebml-webm.bin', bytes.fromhex('1a45dfa3') + b'\x9f\x42\x82\x84webm' + pad)
put('ebml-mkv.bin', bytes.fromhex('1a45dfa3') + b'\x9f\x42\x82\x88matroska' + pad)
put('ebml-other.bin', bytes.fromhex('1a45dfa3') + b'\x9f\x42\x82\x84xxxx' + pad)
put('ebml-short.bin', bytes.fromhex('1a45dfa3'))
put('aac-adts.bin', bytes.fromhex('fff15080') + pad)
put('aac-adts9.bin', bytes.fromhex('fff95080') + pad)
for name, data in [
    ('gzip', b'\x1f\x8b\x08\x00'), ('gzip-9', b'\x1f\x8b\x09\x00'), ('gzip-2', b'\x1f\x8b'),
    ('pdf-lead', b'  %PDF-1.4\n'), ('pdf-nl', b'\n%PDF-1.4\n'), ('pdf-bom', b'\xef\xbb\xbf%PDF-1.4\n'),
    ('pdf-short', b'%PDF-'), ('pdf-junk', b'junk\n%PDF-1.4\n'), ('pdf-binary-junk', b'\x00\x01\x02%PDF-1.4\n'),
    ('pdf-late', b'x' * 1100 + b'%PDF-1.4\n'), ('pdf-text', b'The %PDF- marker is in the text.\n'),
    ('rtf-1', b'{\\rtf1\\ansi hi}'), ('rtf-0', b'{\\rtf0 hi}'), ('rtf-x', b'{\\rtfx hi}'), ('rtf-space', b' {\\rtf1 hi}'),
    ('mz', b'MZ' + pad), ('mz-text', b'MZ is an abbreviation.\n'), ('mz-short', b'MZ'),
    ('class', bytes.fromhex('cafebabe0000003d') + pad), ('class-fat', bytes.fromhex('cafebabe00000002') + pad),
    ('7z', bytes.fromhex('377abcaf271c0004') + pad), ('7z-short', bytes.fromhex('377abcaf271c')),
    ('rar4', b'Rar!\x1a\x07\x00' + pad), ('rar5', b'Rar!\x1a\x07\x01\x00' + pad), ('rar-x', b'Rar!\x1a\x07\x02' + pad),
    ('xcf', b'gimp xcf v003\x00' + pad), ('xcf-file', b'gimp xcf file\x00' + pad),
    ('vtt', b'WEBVTT\n\n00:00.000 --> 00:01.000\nHi\n'), ('vtt-crlf', b'WEBVTT\r\n\r\n00:00.000 --> 00:01.000\r\nHi\r\n'),
    ('vtt-bare', b'WEBVTT'), ('vtt-title', b'WEBVTT - title\n\n'), ('vtt-lower', b'webvtt\n\n'), ('vtt-glued', b'WEBVTTx\n'),
    ('ics', b'BEGIN:VCALENDAR\nVERSION:2.0\nEND:VCALENDAR\n'), ('ics-crlf', b'BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n'),
    ('ics-bare', b'BEGIN:VCALENDAR'), ('ics-other', b'BEGIN:VCARD\nVERSION:3.0\nEND:VCARD\n'),
    ('bmp-text', b'BM' + pad), ('elf', b'\x7fELF\x02\x01\x01' + pad), ('data', bytes(range(1, 200))), ('ff', b'\xff\xff\xff\xff'),
    ('latin1', b'caf\xe9 au lait\n'), ('utf8', 'café au lait\n'.encode()), ('utf8-bom', b'\xef\xbb\xbfHello there.\n'),
    ('utf16', 'Hello there.\n'.encode('utf-16')), ('nul', b'Hello\x00there.\n'), ('tab', b'Hello\tthere.\n'), ('crlf', b'Hello there.\r\nSecond line.\r\n'),
    ('one-nl', b'\n'), ('one-nul', b'\x00'), ('one-ff', b'\xff'), ('one-pct', b'%'), ('two', b'ab'), ('two-nl', b'a\n'), ('two-sp', b'  '),
    ('php', b'<?php echo 1;'), ('php-nl', b'<?php\necho 1;\n'), ('php-only', b'<?php'), ('php-upper', b'<?PHP echo 1;'), ('php-short', b'<? echo 1;'),
    ('php-echo', b'<?= 1 ?>'), ('php-lead', b' <?php echo 1;'), ('php-text', b'Hello\n<?php echo 1;'), ('php-crlf', b'<?php\r\necho 1;\r\n'),
    ('php-tab', b'<?php\techo 1;'), ('php-glued', b'<?phpx echo 1;'), ('php-bom', b'\xef\xbb\xbf<?php echo 1;'), ('php-binary', b'<?php echo 1;\x00\x01\x02'),
    ('cr', b'Hello there.\rSecond line.\r'), ('blank', b'\n\n\n'), ('spaces', b'   '), ('onechar', b'a'), ('nonl', b'Hello there.'),
]:
    put('misc-%s.bin' % name, data)

# Zip layouts, as Office and OpenDocument name their parts.
D, S = zipfile.ZIP_DEFLATED, zipfile.ZIP_STORED


def z(name, entries, method=D, first_stored=None):
    with zipfile.ZipFile(os.path.join(out, name), 'w') as zf:
        for n in entries:
            data = first_stored if n == 'mimetype' and first_stored is not None else b'<x/>' * 3
            zf.writestr(zipfile.ZipInfo(n), data, compress_type=S if n == 'mimetype' else method)


base = ['[Content_Types].xml', '_rels/.rels']
layouts = {
    'word': base + ['word/document.xml'], 'word-rels': base + ['word/_rels/document.xml.rels', 'word/document.xml'],
    'word-docprops': base + ['docProps/app.xml', 'word/document.xml'], 'word-nocore': ['[Content_Types].xml', 'word/document.xml'],
    'word-late': ['[Content_Types].xml', 'docProps/app.xml', 'docProps/core.xml', 'word/document.xml', '_rels/.rels'],
    'word-full': base + ['docProps/app.xml', 'docProps/core.xml', 'word/document.xml'],
    'word-rels-first': ['_rels/.rels', '[Content_Types].xml', 'word/document.xml'],
    'xl': base + ['xl/workbook.xml'], 'xl-rels': base + ['xl/_rels/workbook.xml.rels', 'xl/workbook.xml'],
    'ppt': base + ['ppt/presentation.xml'], 'ppt-rels': base + ['ppt/_rels/presentation.xml.rels', 'ppt/presentation.xml'],
    'visio': base + ['visio/document.xml'], 'plain-ct': base + ['something/else.xml'],
    'xps': base + ['FixedDocSeq.fdseq', 'Documents/1/FixedDoc.fdoc'], 'macro': base + ['word/document.xml', 'word/vbaProject.bin'],
    'two-families': base + ['word/document.xml', 'xl/workbook.xml'],
    'plain': ['a.txt', 'b.txt'], 'one': ['hello.txt'], 'jar': ['META-INF/MANIFEST.MF', 'a/B.class'], 'android': ['AndroidManifest.xml', 'classes.dex'],
    'dirs': ['docs/', 'docs/readme.txt'], 'upper': ['README', 'src/main.c'],
}
for name, entries in layouts.items():
    z('zip-%s.zip' % name, entries)
    z('zip-%s-stored.zip' % name, entries, S)
for name, text in [('odt', b'application/vnd.oasis.opendocument.text'), ('ods', b'application/vnd.oasis.opendocument.spreadsheet'), ('odp', b'application/vnd.oasis.opendocument.presentation'), ('odg', b'application/vnd.oasis.opendocument.graphics'), ('odc', b'application/vnd.oasis.opendocument.chart'), ('odb', b'application/vnd.oasis.opendocument.database'), ('odf', b'application/vnd.oasis.opendocument.formula'), ('ott', b'application/vnd.oasis.opendocument.text-template'), ('epub', b'application/epub+zip'), ('other', b'text/plain'), ('empty', b'')]:
    z('zip-mimetype-%s.zip' % name, ['mimetype', 'content.xml'], first_stored=text)
put('zip-empty.zip', b'PK\x05\x06' + b'\x00' * 18)
put('zip-trunc.zip', b'PK\x03\x04\x14\x00' + b'\x00' * 40)
put('zip-header-only.zip', b'PK\x03\x04\x14\x00\x00\x00\x00\x00' + b'\x00' * 16 + b'\x05\x00\x00\x00hello' + b'\x00' * 30)
tar = bytearray(512)
tar[0:5] = b'a.txt'
tar[100:107] = b'0000644'
tar[124:135] = b'00000000000'
tar[148:156] = b'        '
tar[156] = ord('0')
tar[257:263] = b'ustar\x00'
tar[263:265] = b'00'
tar[148:155] = b'%06o\x00' % sum(tar) + b''
put('tar-ustar.bin', bytes(tar) + b'\x00' * 1024)
bad = bytearray(tar)
bad[148:154] = b'777777'
put('tar-badsum.bin', bytes(bad) + b'\x00' * 1024)
put('tar-short.bin', bytes(tar[:300]))

# Prose of many shapes: libmagic names some of them (mail, code, CSV).
words = 'the quick brown fox jumps over a lazy dog while people read notes about summer gardens and old books'.split()
openers = ['From', 'Subject', 'import', 'package', 'class', 'Dear', 'Notes', 'Chapter', 'Hello', 'Version', 'Path', 'Index', 'diff', 'use', 'Return', 'Date']


def sentence(commas=False):
    parts = [rng.choice(words) for _ in range(rng.randrange(3, 9))]
    if commas and len(parts) > 3:
        parts[2] += ','
    return ' '.join(parts).capitalize() + '.'


for n in range(40):
    lines = [sentence() for _ in range(rng.randrange(1, 6))]
    put('prose-%02d.txt' % n, ('\n'.join(lines) + rng.choice(['', '\n'])).encode())
for n in range(40):
    lines = [sentence(True) for _ in range(rng.randrange(1, 14))]
    put('commas-%02d.txt' % n, ('\n'.join(lines) + rng.choice(['', '\n'])).encode())
for n in range(30):
    cols = rng.randrange(1, 4)
    rows = []
    for r in range(rng.randrange(1, 14)):
        k = cols if rng.random() < 0.85 else cols + 1
        rows.append(','.join(rng.choice(words) for _ in range(k)))
    put('table-%02d.txt' % n, ('\n'.join(rows) + rng.choice(['', '\n'])).encode())
for n in range(20):
    quoted = '\n'.join(['a,"b\nc",d', 'e,f,g', 'h,i,j'][: rng.randrange(1, 4)]) + '\n'
    put('quoted-%02d.txt' % n, quoted.encode())
put('quoted-escape.txt', b'a,"say ""hi""",c\ne,f,g\nh,i,j\n')
put('quoted-open.txt', b'a,"never closed,c\ne,f,g\n')
for n, opener in enumerate(openers):
    put('opener-%02d.txt' % n, (opener + ' ' + sentence() + '\n' + sentence() + '\n').encode())
    put('second-%02d.txt' % n, (sentence() + '\n' + opener + ' ' + sentence() + '\n').encode())
for n, text in enumerate(['A: b', 'x = 1', 'one;two', '#heading', '<b>bold</b>', '{braces}', 'a/b', 'a|b', '100% sure', 'it costs $5', 'mail me@here.com', 'a & b', 'a\\b', '[x]', '~tilde', '`tick`', '*star*', '=eq', '^caret', '@at', 'semi;colon', 'colon: here', 'question?', 'exclaim!', 'quote "x"', "it's", '(paren)', 'dash - here', 'under_score', 'plus + here', 'tag <b>bold</b> here', 'write <me@here.com> now', 'angle < and > only']):
    put('punct-%02d.txt' % n, (sentence() + '\n' + text + '\n').encode())

# Lines that start as code, markup, mail and documents do, first and second.
starts = ['import os', 'package main', 'class Foo', 'def run():', 'function f() {', '#include <stdio.h>', '#!/bin/sh', '<?php echo 1;', '<html>', '<!DOCTYPE html>', '<?xml version="1.0"?>', '{"a": 1}', '[section]', '---', 'diff --git a b', 'Index: x', '@@ -1 +1 @@', 'BEGIN:VCARD', 'From: a@b.c', 'From me@here.com Mon Jan 1 00:00:00 2024', 'Received: from x', 'Date: Mon, 1 Jan 2024', 'To: someone', 'MIME-Version: 1.0', '%!PS-Adobe-3.0', 'SELECT * FROM t;', 'CREATE TABLE t (a int);', 'x = 1', 'key: value', '- item', '* item', '1. item', '> quoted', '| a | b |', 'Dear Sir,', 'Title', '===', 'URL: http://example.com/a', 'Note (see below): done', 'use strict;', 'my $x = 1;', 'local x = 1', 'int main() {', 'public class A {', 'using System;', 'namespace A', 'module Main where', 'require "x"', 'let x = 1', 'var x = 1', 'const x = 1', 'if x then y', 'for i in a; do', 'while true; do', 'echo hello', 'print "hi"', 'return 0', 'Version 1.0', 'Path: a/b', 'begin 644 file', 'end', 'ENDCHAN', 'Copyright (c) 2024', 'Licensed under the MIT License', 'Hello world', '"quoted start"', "'single start'", '(parenthesis start)', '$5 and more', '100 percent', '12:30 meeting', '2024-01-02 note', 'a@b.c', 'http://example.com', 'C:\\Windows', '~/home', '`code`', '*bold*', '_under_', '+plus', '!bang', '&amp;', '^caret', '.hidden', ',comma', ':colon', '?what', '/slash', 'a/b/c', '=equals']
for n, line in enumerate(starts):
    put('start-%02d.txt' % n, (line + '\n' + sentence() + '\n').encode())
    put('later-%02d.txt' % n, (sentence() + '\n' + line + '\n' + sentence() + '\n').encode())
