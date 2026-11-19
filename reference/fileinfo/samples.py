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

# Boundaries of each rule: where a marker may start, how long a header is.
for n in (1000, 1018, 1019, 1020, 1021, 1024, 1025):
    put('edge-pdf-%04d.bin' % n, b'x' * n + b'%PDF-1.4\n')
for n in (2, 3, 4, 8, 9, 10, 11, 20):
    put('edge-gzip-%02d.bin' % n, (b'\x1f\x8b\x08\x00' + b'\0' * 20)[:n])
for n in (6, 7, 8, 9, 20):
    put('edge-7z-%02d.bin' % n, (bytes.fromhex('377abcaf271c0004') + b'\0' * 20)[:n])
for n in (4, 7, 8, 9):
    put('edge-rar-%02d.bin' % n, (b'Rar!\x1a\x07\x01\x00' + b'\0' * 20)[:n])
    put('edge-class-%02d.bin' % n, (bytes.fromhex('cafebabe0000003d') + b'\0' * 20)[:n])
for v in (0x1e, 0x1f, 0x20, 0x2d):
    put('edge-class-v%02x.bin' % v, bytes.fromhex('cafebabe000000') + bytes([v]) + b'\0' * 20)
for n in (505, 511, 512, 513):
    put('edge-tar-%03d.bin' % n, (bytes(tar) + b'\0' * 1024)[:n])
for field in (b'%06o\x00 ', b'%06o \x00', b' %05o\x00 ', b'%07o\x00', b'%06o  ', b'0000000\x00', b'      \x00 '):
    t = bytearray(tar)
    t[148:156] = b'        '
    total = sum(t)
    t[148:156] = (field % total) if b'%' in field else field
    put('edge-tar-field-%s.bin' % field.hex(), bytes(t) + b'\0' * 1024)
t = bytearray(tar)
t[257:263] = b'ustar '
put('edge-tar-gnu.bin', bytes(t) + b'\0' * 1024)
for n in (1010, 1018, 1019, 1020, 1021, 1022, 1030):
    put('edge-vtt-%04d.txt' % n, b'WEBVTT\n\n' + b'x' * (n - 8) + b'-->\n')
for tail in (b'\n', b' ', b'\t', b'\r', b'-', b'x', b''):
    put('edge-vtt-after-%s.txt' % tail.hex(), b'WEBVTT' + tail + b'\n00:00.000 --> 00:01.000\nHi\n')
for sep in (b'\n', b'\r\n', b'\r', b'\n\n', b' ', b''):
    put('edge-ics-%s.txt' % sep.hex(), b'BEGIN:VCALENDAR' + sep + b'VERSION:2.0\nEND:VCALENDAR\n')
for version in (b'1.0', b'2.1', b'2.0'):
    put('edge-ics-v%s.txt' % version.hex(), b'BEGIN:VCALENDAR\nVERSION:' + version + b'\nEND:VCALENDAR\n')
put('edge-ics-lower.txt', b'begin:vcalendar\nversion:2.0\n')
for ctl in (7, 8, 11, 12, 27, 127, 0x85):
    put('edge-control-%02x.txt' % ctl, b'Hello there.\n' + chr(ctl).encode() + b'More text.\n')
for n in (9, 10, 11, 12):
    put('edge-csv-last-%02d.txt' % n, b''.join(b'a,b,c\n' for _ in range(n - 1)) + b'a,b\n')
    put('edge-csv-ok-%02d.txt' % n, b''.join(b'a,b,c\n' for _ in range(n)))
for n in (1, 2, 3):
    put('edge-csv-rows-%d.txt' % n, b''.join(b'a,b\n' for _ in range(n)))
put('edge-csv-nonl.txt', b'a,b\nc,d\ne,f')
put('edge-csv-blank.txt', b'a,b\n\nc,d\n')
put('edge-csv-crlf.txt', b'a,b\r\nc,d\r\ne,f\r\n')
put('edge-csv-first-plain.txt', b'name\na,b\nc,d\n')
put('edge-csv-quote-nl.txt', b'a,"x\ny",b\nc,d,e\nf,g,h\n')
put('edge-csv-quote-end.txt', b'a,b,"c\nd,e\n')
for header in ['from', 'to', 'cc', 'subject', 'date', 'received', 'return-path', 'message-id', 'mime-version', 'reply-to', 'delivered-to', 'index', 'path', 'newsgroups', 'xref', 'begin', 'end', 'diff', 'def', 'From', 'SUBJECT', 'x-mailer', 'lines', 'references', 'organization']:
    put('edge-header-%s.txt' % header, (header + ': value\n' + sentence() + '\n').encode())
    put('edge-keyword-%s.txt' % header, (header + ' value here\n' + sentence() + '\n').encode())
    put('edge-keyword-later-%s.txt' % header, (sentence() + '\n   ' + header + ' value here\n').encode())
for marker in ['#', '[', '{', '<', ';', '@', '%', '\\', '!', '$', '`', '|', '/', '=', '+', '--', '-', '*', '>', '~', '^', '&', '(', '"', "'", '1', '.', ',', ':', '?', '_']:
    tag = '%02x' % ord(marker[0]) + ('x' if len(marker) > 1 else '')
    put('edge-marker-%s.txt' % tag, (marker + 'Hello there friend\n' + sentence() + '\n').encode())
    put('edge-marker-spaced-%s.txt' % tag, (sentence() + '\n  ' + marker + 'Hello there friend\n').encode())
for text in ['a <b> c', 'a <B>', 'a </b>', 'a <!-- c', 'a <?x', 'a <me@here.com> b', 'a <me> b', 'a <1> b', 'a < b', 'a <', 'a <me@x', 'a <b@c>d<e>', 'a {b', 'a }b', 'a \\ b']:
    put('edge-inline-%s.txt' % text.encode().hex(), (sentence() + '\n' + text + '\n').encode())
# MPEG audio headers: the version, layer, bitrate and rate fields.
for second in range(0xe0, 0x100):
    for bitrate in (0, 1, 14, 15):
        for rate in (0, 2, 3):
            third = bitrate << 4 | rate << 2
            put('edge-mpeg-%02x%02x.bin' % (second, third), bytes([0xff, second, third]) + b'\0' * 8)
put('edge-mpeg-short.bin', b'\xff\xfb')
put('edge-mpeg-two.bin', b'\xff\xfb\x90')
for size in (0, 1, 2, 127, 128, 129):
    tag = bytes([0, 0, size >> 7 & 0x7f, size & 0x7f])
    frame = bytes.fromhex('fffb9000')
    for version in (2, 3, 4, 5):
        head = b'ID3' + bytes([version, 0, 0]) + tag
        put('edge-id3-%d-%03d.bin' % (version, size), head + b'\0' * size + frame + b'\0' * 8)
        put('edge-id3-%d-%03d-early.bin' % (version, size), head + b'\0' * max(size - 1, 0) + frame + b'\0' * 8)
        put('edge-id3-%d-%03d-late.bin' % (version, size), head + b'\0' * (size + 1) + frame + b'\0' * 8)
for flags in (0, 0x10, 0x20, 0x40, 0x80, 0xf0):
    put('edge-id3-flags-%02x.bin' % flags, b'ID3\x04\x00' + bytes([flags]) + b'\0\0\0\4' + b'\0' * 4 + bytes.fromhex('fffb9000') + b'\0' * 8)
for bad in (0x80, 0xff):
    for at in range(4):
        size = bytearray(4)
        size[at] = bad
        put('edge-id3-size-%02x-%d.bin' % (bad, at), b'ID3\x04\x00\x00' + bytes(size) + b'\0' * 64)
put('edge-id3-revision.bin', b'ID3\x04\x01\x00\0\0\0\4' + b'\0' * 4 + bytes.fromhex('fffb9000') + b'\0' * 8)
for n in (9, 10, 11):
    put('edge-id3-cut-%02d.bin' % n, (b'ID3\x04\x00\x00\0\0\0\0' + bytes.fromhex('fffb9000'))[:n])
# ISO media atoms: the size field, the atom and the brand.
for size in (0, 1, 7, 8, 9, 12, 20, 0x1000000):
    for atom in (b'ftyp', b'moov', b'wide', b'mdat', b'free', b'skip'):
        put('edge-iso-%s-%08x.bin' % (atom.decode(), size), struct.pack('>I', size) + atom + b'isom\0\0\2\0' + b'\0' * 16)
for n in (7, 8, 11, 12):
    put('edge-iso-cut-%02d.bin' % n, (struct.pack('>I', 20) + b'ftypisom' + b'\0' * 12)[:n])
# Ogg: the first page's header fields and where its payload starts.
good = b'OggS\x00\x02' + b'\x00' * 20 + b'\x01' + b'\x1e' + b'\x01vorbis' + b'\x00' * 24
put('edge-ogg-good.bin', good)
for at, value in ((4, 1), (5, 0), (5, 1), (5, 4), (5, 3), (26, 0), (26, 2), (27, 0), (28, 0x02), (28, 0x00)):
    changed = bytearray(good)
    changed[at] = value
    put('edge-ogg-%d-%02x.bin' % (at, value), bytes(changed))
for n in (5, 6, 26, 27, 28, 29, 35):
    put('edge-ogg-cut-%02d.bin' % n, good[:n])
put('edge-ogg-late.bin', good[:27] + b'\0' + good[27:])
# EBML: where in the first 64 bytes the DocType sits.
for n in (0, 8, 50, 52, 53, 54, 55, 56, 57, 58, 59, 60, 64):
    put('edge-ebml-webm-%02d.bin' % n, bytes.fromhex('1a45dfa3') + b'\0' * n + b'\x42\x82\x84webm' + b'\0' * 80)
    put('edge-ebml-mkv-%02d.bin' % n, bytes.fromhex('1a45dfa3') + b'\0' * n + b'\x42\x82\x88matroska' + b'\0' * 80)
put('edge-ebml-split.bin', bytes.fromhex('1a45dfa3') + b'\x42\x82\x84web')
# MPEG programs and sequences, ADTS and the like.
for tag in (0xb2, 0xb3, 0xb4, 0xb9, 0xba, 0xbb, 0xbc, 0xe0, 0x00, 0x01):
    put('edge-mpeg-ps-%02x.bin' % tag, bytes([0, 0, 1, tag]) + b'\0' * 16)
for second in (0xf0, 0xf1, 0xf2, 0xf3, 0xf8, 0xf9, 0xfa, 0xe0, 0xe1):
    put('edge-adts-%02x.bin' % second, bytes([0xff, second, 0x50, 0x80]) + b'\0' * 16)
# FLV, RIFF, the ASF GUID, MIDI, FLAC at their lengths.
for n in (3, 4, 5, 9):
    put('edge-flv-%d.bin' % n, (b'FLV\x01\x05\0\0\0\x09' + b'\0' * 8)[:n])
for version in (0, 2, 3):
    put('edge-flv-v%d.bin' % version, b'FLV' + bytes([version]) + b'\x05\0\0\0\x09' + b'\0' * 8)
for n in (4, 8, 11, 12):
    put('edge-riff-%02d.bin' % n, (b'RIFF\x10\0\0\0WAVEfmt ' + b'\0' * 16)[:n])
for n in (3, 4, 15, 16):
    put('edge-asf-%02d.bin' % n, bytes.fromhex('3026b2758e66cf11a6d900aa0062ce6c')[:n])
for n in (3, 4):
    put('edge-midi-%d.bin' % n, b'MThd\0\0\0\6'[:n])
    put('edge-flac-%d.bin' % n, b'fLaC\0\0\0\0'[:n])
# RTF: the digit after the control word.
for ch in (b'1', b'0', b'9', b'x', b' ', b''):
    put('edge-rtf-%s.bin' % ch.hex(), b'{\\rtf' + ch + b'\\ansi hi}')
put('edge-rtf-upper.bin', b'{\\RTF1 hi}')
# PHP's tag and XCF: the case and what follows.
for text in (b'<?php', b'<?PHP', b'<?Php', b'<?ph', b'<?phq', b'<?php\n', b'<? php'):
    put('edge-php-%s.txt' % text.hex(), text + b' echo 1;\n')
for text in (b'gimp xcf v003\0', b'gimp xcf file\0', b'gimp xcf ', b'gimp xcf', b'GIMP XCF v003\0'):
    put('edge-xcf-%s.bin' % text.hex(), text + b'\0' * 20)
for n in (50, 200, 500, 900, 1000, 1010, 1020, 1030, 1100, 2000, 5000):
    put('edge-vtt-cue-%04d.txt' % n, b'WEBVTT\n\nNOTE ' + b'x' * n + b'\n\n00:00.000 --> 00:01.000\nHi\n')
for i, cue in enumerate((b'00:00.000 --> 00:01.000', b'00:00:00.000 --> 00:00:01.000', b'0:0.0 --> 0:1.0', b'00:00.000-->00:01.000', b'a --> b', b'1 --> 2', b'00:00.000 --> 00:01.000 align:start', b'00:00 --> 00:01', b'00:00.000 --> 00:01', b'00:00 --> 00:01.000', b'00:00.000  --> 00:01.000', b'00:00.000 -->  00:01.000', b'00:00.000\t-->\t00:01.000', b'00:00:00:00.000 --> 00:01.000', b'000:00.000 --> 00:01.000', b'00:00.00 --> 00:01.00', b'00:00.0000 --> 00:01.0000', b'59:59.999 --> 99:99.999', b'00:60.000 --> 00:01.000')):
    put('edge-vtt-timing-%02d.txt' % i, b'WEBVTT\n\n' + cue + b'\nHi\n')
for n in tuple(range(240, 258)) + tuple(range(260, 520, 20)):
    put('edge-vtt-window-%03d.txt' % n, b'WEBVTT\n\nNOTE ' + b'x' * (n - 14) + b'\n\n00:00.000 --> 00:01.000\nHi\n')
for pre in (b'', b'\n', b' ', b'\xef\xbb\xbf'):
    put('edge-vtt-before-%s.txt' % pre.hex(), pre + b'WEBVTT\n\n00:00.000 --> 00:01.000\nHi\n')
for n in (1000, 1020, 1025, 1030):
    put('edge-pdf-text-%04d.txt' % n, b'Some words of text. ' * (n // 20) + b'%PDF-1.4\n')

# ZIP entries with extra fields and comments, so a name's place in the
# central directory depends on the sizes before it; and quotes with a
# newline after an escaped one.
def zx(name, entries, extra=b'', comment=b''):
    with zipfile.ZipFile(os.path.join(out, name), 'w') as zf:
        for n in entries:
            info = zipfile.ZipInfo(n)
            info.extra = extra
            info.comment = comment
            zf.writestr(info, b'<x/>' * 3, compress_type=D)


field = struct.pack('<HH', 0xcafe, 4) + b'abcd'
for tag, extra, comment in (('extra', field, b''), ('comment', b'', b'a comment here'), ('both', field, b'a comment here')):
    zx('zip-spread-%s-word.zip' % tag, ['[Content_Types].xml', '_rels/.rels', 'docProps/app.xml', 'word/document.xml'], extra, comment)
    zx('zip-spread-%s-xl.zip' % tag, ['[Content_Types].xml', 'docProps/core.xml', 'docProps/app.xml', 'xl/workbook.xml'], extra, comment)
    zx('zip-spread-%s-plain.zip' % tag, ['one.txt', 'two.txt', 'three.txt', 'four.txt'], extra, comment)
    zx('zip-spread-%s-meta.zip' % tag, ['a.txt', 'b.txt', 'META-INF/MANIFEST.MF'], extra, comment)
put('quoted-escape-nl.txt', b'a,"x""\ny",b\nc,d\ne,f\n')
put('quoted-escape-two.txt', b'a,"x""""y",b\nc,d,e\nf,g,h\n')
put('quoted-escape-end.txt', b'a,"x""",b\nc,d,e\nf,g,h\n')
