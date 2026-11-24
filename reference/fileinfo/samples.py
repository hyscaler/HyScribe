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
    if name.endswith(('.jpg', '.png', '.gif', '.webp', '.avif', '.bmp', '.tif')) or (os.path.getsize(path) > 30000 and name != 'sample.ppt'):
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


# Compound File Binary files (Word, Excel and PowerPoint 97 to 2003 and kin):
# built here with the structures libmagic reads (header, FAT, mini FAT,
# directory, short stream, summary information), each Office kind, and the
# damage it must leave alone.
CFB_END, CFB_FREE, CFB_FATSEC, CFB_NIL = 0xFFFFFFFE, 0xFFFFFFFF, 0xFFFFFFFD, 0xFFFFFFFF
CFB_SIGNATURE = bytes.fromhex('d0cf11e0a1b11ae1')
SUMMARY = '\x05SummaryInformation'
DOC_SUMMARY = '\x05DocumentSummaryInformation'
MSI_CLASS = bytes.fromhex('84100c0000000000c000000000000046')
WORD_CLASS = bytes.fromhex('0609020000000000c000000000000046')


def lpstr(text, kind=0x1E):
    raw = text if isinstance(text, bytes) else text.encode('latin1')
    return struct.pack('<II', kind, len(raw) + 1) + raw + b'\0'


def propset(props, app=None, trim=0, pad=True):
    """A summary information stream of one section: props is a list of
    (id, type, value bytes); the last value is not padded to four bytes and
    the section ends `trim` bytes before it, as the boundary cases need."""
    items = [(1, struct.pack('<HH', 2, 0) + struct.pack('<I', 1252))]
    items += [(pid, struct.pack('<I', ty) + value) for pid, ty, value in props]
    if app is not None:
        items.append((0x12, lpstr(app)))
    base = 8 + 8 * len(items)
    table, body = b'', b''
    for at, (pid, value) in enumerate(items):
        table += struct.pack('<II', pid, base + len(body))
        body += value + (b'\0' * (-len(value) % 4) if pad and at + 1 < len(items) else b'')
    section = struct.pack('<II', base + len(body) - trim, len(items)) + table + body
    head = struct.pack('<HHI', 0xFFFE, 0, 0x00020006) + b'\0' * 16 + struct.pack('<I', 1)
    return head + bytes.fromhex('e0859ff2f94f6810ab9108002b27b3d9') + struct.pack('<I', 48) + section


class Cfb:
    """A Compound File Binary file of version 3: the bytes, and where its
    parts are (sector numbers) so cases can damage them."""

    def __init__(self, streams, clsid=b'\0' * 16, data_first=False, cyclic=None, kinds=None):
        sector, short = 512, 64
        kinds = kinds or {}
        big = [(n, d) for n, d in streams if len(d) >= 4096]
        small = [(n, d) for n, d in streams if len(d) < 4096]
        mini, minifat, start = b'', [], {}
        for n, d in small:
            first, count = len(mini) // short, (len(d) + short - 1) // short
            start[n] = first if count else CFB_END
            minifat += [first + i + 1 if i < count - 1 else CFB_END for i in range(count)]
            mini += d + b'\0' * (-len(d) % short)
        ndir = (len(streams) + 1 + 3) // 4
        nminifat = (len(minifat) * 4 + sector - 1) // sector
        nmini = (len(mini) + sector - 1) // sector
        nbig = {n: (len(d) + sector - 1) // sector for n, d in big}
        body = ndir + nminifat + nmini + sum(nbig.values())
        nfat = 1
        while nfat * 128 < body + nfat:
            nfat += 1
        fat = [CFB_FREE] * (nfat * 128)

        def link(first, count):
            for i in range(count):
                fat[first + i] = first + i + 1 if i < count - 1 else CFB_END

        parts, at = {}, 0

        def place_big():
            nonlocal at
            for n, d in big:
                start[n] = at
                link(at, nbig[n])
                if cyclic == n:
                    fat[at + nbig[n] - 1] = at
                parts[at] = d
                at += nbig[n]

        if data_first:
            place_big()
        else:
            at = nfat
        self.dir_first = at
        link(at, ndir)
        at += ndir
        self.minifat_first = at if nminifat else CFB_END
        if nminifat:
            link(at, nminifat)
        at += nminifat
        self.mini_first = at if nmini else CFB_END
        if nmini:
            link(at, nmini)
        at += nmini
        if not data_first:
            place_big()
        self.fat_sectors = list(range(at, at + nfat)) if data_first else list(range(nfat))
        for i in self.fat_sectors:
            fat[i] = CFB_FATSEC
        self.fat = fat
        self.minifat = minifat
        self.start = start
        self.names = [n for n, _ in streams]

        def entry(name, kind, left, right, child, cls, first, size):
            raw = name.encode('utf-16-le')[:62] + b'\0\0'
            return (raw + b'\0' * (64 - len(raw)) + struct.pack('<HBBIII', len(raw), kind, 1, left, right, child)
                    + cls + struct.pack('<IQQIQ', 0, 0, 0, first, size))

        entries = [entry('Root Entry', 5, CFB_NIL, CFB_NIL, 1 if streams else CFB_NIL, clsid,
                         self.mini_first if nmini else CFB_END, len(mini))]
        for i, (n, d) in enumerate(streams):
            right = i + 2 if i + 1 < len(streams) else CFB_NIL
            entries.append(entry(n, kinds.get(n, 2), CFB_NIL, right, CFB_NIL, b'\0' * 16, start[n], len(d)))
        entries += [b'\0' * 128] * (-len(entries) % 4)
        self.entries = len(entries)
        self.header = bytearray(512)
        h = self.header
        h[0:8] = CFB_SIGNATURE
        struct.pack_into('<HHHHH', h, 0x18, 0x3E, 3, 0xFFFE, 9, 6)
        struct.pack_into('<IIIIIIIII', h, 0x28, 0, nfat, self.dir_first, 0, 4096, self.minifat_first, nminifat, CFB_END, 0)
        for i in range(109):
            struct.pack_into('<I', h, 0x4c + 4 * i, self.fat_sectors[i] if i < nfat else CFB_FREE)
        self.sectors = {}
        self.sectors[self.dir_first] = b''.join(entries)
        if nminifat:
            raw = b''.join(struct.pack('<I', x) for x in minifat)
            self.sectors[self.minifat_first] = raw + b'\xff' * (-len(raw) % sector)
        if nmini:
            self.sectors[self.mini_first] = mini + b'\0' * (-len(mini) % sector)
        self.sectors.update({s: d + b'\0' * (-len(d) % sector) for s, d in parts.items()})
        self.total = at + nfat

    def bytes(self, fat=None):
        fat = self.fat if fat is None else fat
        out = [b'\0' * 512] * self.total
        raw = b''.join(struct.pack('<I', x) for x in fat)
        for i, s in enumerate(self.fat_sectors):
            out[s] = raw[i * 512:(i + 1) * 512]
        for s, d in self.sectors.items():
            for i in range(len(d) // 512):
                out[s + i] = d[i * 512:(i + 1) * 512]
        return bytes(self.header) + b''.join(out)

    def entry_at(self, index):
        """Where directory entry `index` starts, in the file."""
        return 512 * (self.dir_first + 1) + 128 * index


def cfb(streams, **options):
    return Cfb(streams, **options)


def put_cfb(name, c, patch=None):
    data = bytearray(c.bytes())
    for at, fmt, value in patch or []:
        struct.pack_into(fmt, data, at, value)
    put('cfb-%s.bin' % name, bytes(data))


WORD = bytes.fromhex('eca5c100') + b'\0' * 60
BOOK = bytes.fromhex('0908100000060500') + b'\0' * 60
SLIDES = bytes.fromhex('0000f0030800000000') + b'\0' * 60
SI = lambda app=None, props=(), **o: (SUMMARY, propset(list(props), app, **o))

# Each Office kind, by application name, by stream name and by both.
for tag, stream, apps in [
        ('word', ('WordDocument', WORD), ['Microsoft Word', 'Microsoft Office Word', 'LibreOffice/7.6']),
        ('excel', ('Workbook', BOOK), ['Microsoft Excel', 'Microsoft Office Excel', 'LibreOffice/7.6']),
        ('book', ('Book', BOOK), ['Microsoft Excel 4.0']),
        ('slides', ('PowerPoint Document', SLIDES), ['Microsoft PowerPoint', 'Microsoft Office PowerPoint', 'LibreOffice/7.6']),
        ('project', ('Props', b'x' * 70), ['Microsoft Project', 'Microsoft Office Project'])]:
    put_cfb('%s-nosummary' % tag, cfb([stream]))
    put_cfb('%s-emptysummary' % tag, cfb([stream, SI()]))
    for i, app in enumerate(apps):
        put_cfb('%s-app%d' % (tag, i), cfb([stream, SI(app)]))
        put_cfb('%s-app%d-first' % (tag, i), cfb([SI(app), stream]))

# The application names the summary information is read for, and the
# wrong ones: the table's order, case, and where a word sits in the name.
for i, app in enumerate(['word', 'WORD', 'wOrD', 'Microsoft Excel', 'EXCEL', 'excel 97', 'powerpoint', 'PowerPoint', 'POWERPOINT',
                         'Crystal Reports', 'crystal reports 9', 'Advanced Installer', 'InstallShield', 'Microsoft Patch Compiler',
                         'NAnt', 'nant 0.9', 'Windows Installer', 'Acrobat', 'Wor', 'Wo rd', 'Microsoft Offic', 'Word Excel',
                         'PowerPoint Excel', 'Excel PowerPoint', 'Excel Word', 'Windows', 'Installer', 'Crystal', 'Microsoft Patch',
                         'xWordx', 'sword', 'Microsoft Office Word 2003', '']):
    put_cfb('app-%02d' % i, cfb([('Other', b'x' * 70), SI(app)]))
put_cfb('app-nonprintable', cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1E, struct.pack('<I', 8) + b'W\x01o\x02rd\0\0')]))]))
put_cfb('app-high', cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1E, struct.pack('<I', 8) + b'W\xe9ord\0\0\0')]))]))
put_cfb('app-nul', cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1E, struct.pack('<I', 8) + b'Wo\0rd\0\0\0')]))]))
put_cfb('app-nul-first', cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1E, struct.pack('<I', 8) + b'\0Word\0\0\0')]))]))
put_cfb('app-del', cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1E, struct.pack('<I', 8) + b'Wo\x7frd\0\0\0')]))]))
put_cfb('app-space', cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1E, struct.pack('<I', 8) + b'Word \0\0\0')]))]))
for n in (0, 1, 2, 3, 4, 5):
    put_cfb('app-len%d' % n, cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1E, struct.pack('<I', n) + b'Word\0\0\0\0')]))]))
for n in (7, 8, 9, 10):
    # A wide string: its length is read as bytes, every other one taken.
    put_cfb('app-wide-%d' % n, cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1F, struct.pack('<I', n) + 'Word\0\0\0\0'.encode('utf-16-le'))]))]))
put_cfb('app-wide-nul', cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1F, struct.pack('<I', 10) + 'Wo\0rd\0'.encode('utf-16-le'))]))]))
put_cfb('app-wide-high', cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1F, struct.pack('<I', 10) + 'Wšord\0'.encode('utf-16-le'))]))]))
for n in (1015, 1018, 1019, 1020, 1023, 1024, 1030):
    put_cfb('app-long-%04d' % n, cfb([('Other', b'x' * 70), SI('x' * n + 'Word')]))
for n in (1019, 1020, 1021, 1022, 1023, 1025):
    put_cfb('app-long-lead-%04d' % n, cfb([('Other', b'x' * 70), SI('x' * (n - 4) + 'Word' + 'x' * 30)]))
put_cfb('app-wrong-id', cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x11, 0x1E, struct.pack('<I', 5) + b'Word\0\0\0\0')]))]))
put_cfb('app-id-13', cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x13, 0x1E, struct.pack('<I', 5) + b'Word\0\0\0\0')]))]))
put_cfb('app-twice', cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1E, struct.pack('<I', 6) + b'Excel\0\0\0')], app='Microsoft Word'))]))
put_cfb('app-twice-none', cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1E, struct.pack('<I', 5) + b'Acro\0\0\0\0')], app='Microsoft Word'))]))
put_cfb('app-twice-empty', cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1E, struct.pack('<I', 1) + b'\0\0\0\0')], app='Microsoft Word'))]))
put_cfb('app-unicode-lead', cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1E, struct.pack('<I', 7) + b'\xe9Word\0\0')]))]))
put_cfb('app-in-title', cfb([('Other', b'x' * 70), (SUMMARY, propset([(2, 0x1E, struct.pack('<I', 5) + b'Word\0\0\0\0')]))]))
# The root's class: Windows Installer wins over the application name.
put_cfb('class-msi', cfb([('Other', b'x' * 70), SI('Microsoft Word')], clsid=MSI_CLASS))
put_cfb('class-msi-none', cfb([('Other', b'x' * 70), SI()], clsid=MSI_CLASS))
put_cfb('class-msi-names', cfb([('WordDocument', WORD), SI()], clsid=MSI_CLASS))
put_cfb('class-msi-nosummary', cfb([('WordDocument', WORD)], clsid=MSI_CLASS))
put_cfb('class-word', cfb([('Other', b'x' * 70), SI()], clsid=WORD_CLASS))
put_cfb('class-word-names', cfb([('Other', b'x' * 70), SI('Microsoft Word')], clsid=WORD_CLASS))
c = cfb([('Other', b'x' * 70), SI()], clsid=MSI_CLASS)
for at in (0, 7, 8, 15):
    put_cfb('class-msi-bit-%02d' % at, c, [(c.entry_at(0) + 80 + at, '<B', 0x55)])

# What the entries' names give: the first entry whose name holds a word.
for i, name in enumerate(['Book', 'Workbook', 'MyBookmarks', 'bOOk', 'WordDocument', 'WORDDOCUMENT', 'XWordDocumentX', 'WordDocumen',
                          'PowerPoint', 'PowerPoint Document', 'powerpoint', 'DigitalSignature', 'xdigitalsignaturex', 'Digital',
                          'Root', 'Boo', 'Doc', 'Workboo', 'Pow', '\x01CompObj', 'Macros', 'Data', 'a' * 31, 'WordDocument' * 2,
                          'Book' * 7, 'abcdefghijklmnopqrstuvwxyzWord12', 'abcdefghijklmnopqrstuvwxyzWo1234']):
    put_cfb('name-%02d' % i, cfb([(name, b'x' * 70), SI()]))
    put_cfb('name-%02d-app' % i, cfb([(name, b'x' * 70), SI('Acrobat')]))
put_cfb('name-order-word-excel', cfb([('WordDocument', WORD), ('Workbook', BOOK), SI()]))
put_cfb('name-order-excel-word', cfb([('Workbook', BOOK), ('WordDocument', WORD), SI()]))
put_cfb('name-order-ppt-word', cfb([('PowerPoint Document', SLIDES), ('WordDocument', WORD), SI()]))
put_cfb('name-order-sig-word', cfb([('DigitalSignature', b'x' * 70), ('WordDocument', WORD), SI()]))
put_cfb('name-order-summary-first', cfb([SI(), ('WordDocument', WORD)]))
put_cfb('name-storage', cfb([('WordDocument', WORD), SI()], kinds={'WordDocument': 1}))
put_cfb('name-empty-entry', cfb([('', b'x' * 70), ('WordDocument', WORD), SI()]))
c = cfb([('Other', b'x' * 70), SI()])
unterminated = [(c.entry_at(1) + 2 * i, '<H', 0x41 + i) for i in range(32)]
put_cfb('name-unterminated', c, unterminated)
put_cfb('name-unterminated-word', c, [(c.entry_at(1) + 2 * i, '<H', ord('WordDocument'[i]) if i < 12 else 0x41) for i in range(32)])
put_cfb('name-high-byte', c, [(c.entry_at(1), '<H', 0x0100 | ord('W')), (c.entry_at(1) + 2, '<H', ord('o'))])
put_cfb('name-high-nul', c, [(c.entry_at(1), '<H', 0x0100)])
put_cfb('name-wide-book', c, [(c.entry_at(1) + 2 * i, '<H', 0x0100 | ord('Book'[i])) for i in range(4)] + [(c.entry_at(1) + 8, '<H', 0)])
put_cfb('name-root-word', c, [(c.entry_at(0) + 2 * i, '<H', ord('WordDocument'[i])) for i in range(12)] + [(c.entry_at(0) + 24, '<H', 0)])
put_cfb('name-late-entry', c, [(c.entry_at(3) + 2 * i, '<H', ord('WordDocument'[i])) for i in range(12)] + [(c.entry_at(3) + 24, '<H', 0), (c.entry_at(3) + 66, '<B', 2)])

# Without summary information, the streams a file holds name it, in order.
for i, names in enumerate([['Book'], ['Workbook'], ['WordDocument'], ['PowerPoint'], ['PowerPoint Document'], ['EncryptedPackage'],
                           ['EncryptedSummary'], ['mfbu_header'], ['__properties_version1.0'], ['__recip_version1.0_#00000000'],
                           ['Catalog'], ['Foo'], ['WordDocument', 'Workbook'], ['Workbook', 'WordDocument'], ['PowerPoint', 'WordDocument'],
                           ['EncryptedPackage', 'WordDocument'], ['WordDocument', 'EncryptedPackage'], ['mfbu_header', 'Workbook'],
                           ['__properties_version1.0', 'WordDocument'], ['WordDocument', '__properties_version1.0'], ['workbook'],
                           ['wordDocument'], ['WordDocument ']]):
    put_cfb('streams-%02d' % i, cfb([(n, b'x' * 70) for n in names]))
put_cfb('streams-outlook-storage', cfb([('__recip_version1.0_#00000000', b'x' * 70)], kinds={'__recip_version1.0_#00000000': 1}))
put_cfb('streams-outlook-stream', cfb([('__recip_version1.0_#00000000', b'x' * 70)]))
put_cfb('streams-word-storage', cfb([('WordDocument', b'x' * 70)], kinds={'WordDocument': 1}))
put_cfb('streams-word-big', cfb([('WordDocument', b'x' * 5000)]))
put_cfb('streams-word-big-first', cfb([('WordDocument', b'x' * 5000)], data_first=True))
put_cfb('streams-none', cfb([]))
put_cfb('streams-two-summaries', cfb([(SUMMARY, propset([], 'Microsoft Excel')), (SUMMARY, propset([], 'Microsoft Word'))]))
put_cfb('streams-two-summaries-rev', cfb([(SUMMARY, propset([], 'Microsoft Word')), (SUMMARY, propset([], 'Microsoft Excel'))]))
put_cfb('streams-two-words', cfb([('WordDocument', b'x' * 70), ('Other', b'x' * 70), ('WordDocument', b'x' * 70)]))
put_cfb('streams-summary-storage', cfb([('WordDocument', WORD), (SUMMARY, propset([], 'Microsoft Excel'))], kinds={SUMMARY: 1}))

# The document summary information stands in where the summary information
# is missing, and is read the same way.
put_cfb('docsum-app', cfb([('Other', b'x' * 70), (DOC_SUMMARY, propset([], 'Microsoft Word'))]))
put_cfb('docsum-none', cfb([('Other', b'x' * 70), (DOC_SUMMARY, propset([]))]))
put_cfb('docsum-names', cfb([('WordDocument', WORD), (DOC_SUMMARY, propset([]))]))
put_cfb('docsum-both', cfb([(SUMMARY, propset([], 'Microsoft Excel')), (DOC_SUMMARY, propset([], 'Microsoft Word'))]))
put_cfb('docsum-both-none', cfb([('Other', b'x' * 70), (SUMMARY, propset([])), (DOC_SUMMARY, propset([], 'Microsoft Word'))]))
put_cfb('docsum-broken', cfb([('Other', b'x' * 70), (DOC_SUMMARY, b'x' * 70)]))
put_cfb('docsum-vector', cfb([('Other', b'x' * 70), (DOC_SUMMARY, propset([(0x0c, 0x101E, struct.pack('<II', 1, 5) + b'Word\0\0\0\0')]))]))

# The types of the other properties: those libmagic prints, and those it
# fails on, with the application name before or after them.
for tag, ty, value in [('i2', 2, struct.pack('<H', 1)), ('i4', 3, struct.pack('<I', 1)), ('r4', 4, struct.pack('<I', 1)),
                       ('r8', 5, struct.pack('<Q', 1)), ('ui4', 0x13, struct.pack('<I', 1)), ('filetime', 0x40, struct.pack('<Q', 1)),
                       ('filetime-big', 0x40, struct.pack('<Q', 1700000000 * 10000000 + 116444736000000000)),
                       ('null', 1, b''), ('clipboard', 0x47, struct.pack('<I', 4) + b'abcd'), ('lpstr', 0x1E, lpstr('x', 0)[4:]),
                       ('lpwstr', 0x1F, struct.pack('<I', 4) + 'x\0'.encode('utf-16-le')),
                       ('bool', 0x0B, struct.pack('<I', 1)), ('i8', 0x14, struct.pack('<Q', 1)), ('empty', 0, b''),
                       ('blob', 0x41, struct.pack('<I', 0)), ('unknown', 0x1234, struct.pack('<I', 0)),
                       ('vector', 0x1003, struct.pack('<II', 1, 5)), ('byref', 0x4003, struct.pack('<I', 1)),
                       ('array', 0x2003, struct.pack('<I', 1)), ('reserved', 0x8003, struct.pack('<I', 1)),
                       ('i1', 0x10, struct.pack('<I', 1)), ('date', 7, struct.pack('<Q', 1)), ('error', 0x0A, struct.pack('<I', 1)),
                       ('ui2', 0x12, struct.pack('<I', 1)), ('ui8', 0x15, struct.pack('<Q', 1)), ('cy', 6, struct.pack('<Q', 1)),
                       ('bstr', 8, struct.pack('<II', 1, 0))]:
    put_cfb('type-%s' % tag, cfb([('Other', b'x' * 70), SI('Microsoft Word', [(0x07, ty, value)])]))
    put_cfb('type-%s-after' % tag, cfb([('Other', b'x' * 70), SI(None, [(0x7f, ty, value)])]))
    put_cfb('type-%s-alone' % tag, cfb([('Other', b'x' * 70), SI(None, [(0x07, ty, value)])]))
    put_cfb('type-%s-names' % tag, cfb([('WordDocument', WORD), SI(None, [(0x07, ty, value)])]))
    put_cfb('type-%s-app-names' % tag, cfb([('Workbook', BOOK), SI('Microsoft Word', [(0x07, ty, value)])]))

# A value that fits its section to the byte, and the same one a byte short.
for tag, ty, value in [('i2', 2, struct.pack('<H', 1)), ('i4', 3, struct.pack('<I', 1)), ('r8', 5, struct.pack('<Q', 1)),
                       ('filetime', 0x40, struct.pack('<Q', 1)), ('null', 1, b''), ('clipboard', 0x47, b''), ('lpstr', 0x1E, struct.pack('<I', 2) + b'x\0')]:
    for trim in (0, 1):
        put_cfb('fit-%s-%d' % (tag, trim), cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x07, ty, value)], trim=trim))]))
        put_cfb('fit-%s-%d-names' % (tag, trim), cfb([('WordDocument', WORD), (SUMMARY, propset([(0x07, ty, value)], trim=trim))]))
for n in (1, 2, 3, 4, 5, 6):
    put_cfb('fit-string-%d' % n, cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1E, struct.pack('<I', n) + b'Word\0\0\0\0'[:n])]))]))
    put_cfb('fit-string-%d-trim' % n, cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1E, struct.pack('<I', n) + b'Word\0\0\0\0'[:n])], trim=1))]))
put_cfb('fit-app-exact', cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1E, struct.pack('<I', 4) + b'Word')]))]))
put_cfb('fit-app-exact-trim', cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1E, struct.pack('<I', 4) + b'Word')], trim=1))]))
put_cfb('fit-app-over', cfb([('Other', b'x' * 70), (SUMMARY, propset([(0x12, 0x1E, struct.pack('<I', 40) + b'Word')]))]))

# The summary stream's own layout: where its section is and how long.
base = propset([], 'Microsoft Word')
sumcase = lambda name, data, streams=(): put_cfb(name, cfb(list(streams) + [(SUMMARY, data)]))
for n in (0, 8, 16, 28, 40, 47, 48):
    sumcase('sum-cut-%02d' % n, base[:n], [('WordDocument', WORD)])
for tag, offset in [('zero', 0), ('in-header', 0x20), ('at-end', len(base)), ('past-end', len(base) + 4), ('huge', 0x7fffffff), ('wrap', 0xfffffff8),
                    ('into-table', 0x34), ('last-byte', len(base) - 1), ('eight-from-end', len(base) - 8), ('seven-from-end', len(base) - 7)]:
    data = bytearray(base)
    struct.pack_into('<I', data, 0x2c, offset)
    sumcase('sum-offset-%s' % tag, bytes(data), [('WordDocument', WORD)])
for tag, at, value in [('len-zero', 0x30, 0), ('len-seven', 0x30, 7), ('len-eight', 0x30, 8), ('len-nine', 0x30, 9), ('len-short', 0x30, len(base) - 0x30 - 1),
                       ('len-exact', 0x30, len(base) - 0x30), ('len-long', 0x30, len(base) - 0x30 + 1), ('len-huge', 0x30, 0xffffffff),
                       ('count-zero', 0x34, 0), ('count-one', 0x34, 1), ('count-many', 0x34, 1000), ('count-huge', 0x34, 0xffffffff),
                       ('prop-offset-low', 0x3c, 7), ('prop-offset-eight', 0x3c, 8), ('prop-offset-table', 0x3c, 16), ('prop-offset-huge', 0x3c, 0xffffffff),
                       ('prop-offset-end', 0x3c, len(base) - 0x30), ('prop-offset-last', 0x3c, len(base) - 0x30 - 1),
                       ('prop-offset-near', 0x3c, len(base) - 0x30 - 4), ('prop-offset-near-three', 0x3c, len(base) - 0x30 - 3)]:
    data = bytearray(base)
    struct.pack_into('<I', data, at, value)
    sumcase('sum-%s' % tag, bytes(data), [('WordDocument', WORD)])
    sumcase('sum-%s-other' % tag, bytes(data), [('Other', b'x' * 70)])
sumcase('sum-trailing', base + b'\0' * 500, [('Other', b'x' * 70)])
# A section that runs to the last byte of the short sectors, and one past.
for tag, extra in (('to-end', 0), ('past-end', 1)):
    data = bytearray(base + b'\0' * (-len(base) % 64))
    struct.pack_into('<I', data, 0x30, len(data) - 0x30 + extra)
    sumcase('sum-len-%s' % tag, bytes(data), [('Other', b'x' * 70)])
sumcase('sum-big', propset([(0x07, 0x1E, lpstr('x' * 4200)[4:])], 'Microsoft Word'), [('Other', b'x' * 70)])
sumcase('sum-big-none', propset([(0x07, 0x1E, lpstr('x' * 4200)[4:])]), [('WordDocument', WORD)])
sumcase('sum-two-sections', base[:44] + struct.pack('<I', 48) + base[48:], [('Other', b'x' * 70)])
sumcase('sum-empty-section', base[:0x30] + struct.pack('<II', 8, 0), [('Other', b'x' * 70)])
sumcase('sum-empty-section-padded', base[:0x30] + struct.pack('<II', 12, 0) + b'\0' * 4, [('WordDocument', WORD)])
sumcase('sum-bad-order', b'\xff\xff' + base[2:], [('Other', b'x' * 70)])
sumcase('sum-bad-class', base[:12] + b'\xff' * 16 + base[28:], [('Other', b'x' * 70)])
sumcase('sum-bad-format', base[:28] + b'\xff' * 16 + base[44:], [('Other', b'x' * 70)])

# The Hangul Word Processor's header and the others of that name.
for tag, data in [('hwp', b'HWP Document File' + b'\0' * 100), ('hwp-short', b'HWP Document Fil'), ('hwp-lower', b'hwp document file' + b'\0' * 100),
                  ('hwp-late', b'x' + b'HWP Document File' + b'\0' * 100), ('hwp-exact', b'HWP Document File'), ('other', b'x' * 100),
                  ('hwp-big', b'HWP Document File' + b'\0' * 5000)]:
    put_cfb('fileheader-%s' % tag, cfb([('FileHeader', data), ('WordDocument', WORD), SI('Microsoft Word')]))
    put_cfb('fileheader-%s-nosummary' % tag, cfb([('FileHeader', data), ('WordDocument', WORD)]))
put_cfb('fileheader-storage', cfb([('FileHeader', b'HWP Document File' + b'\0' * 100), ('WordDocument', WORD)], kinds={'FileHeader': 1}))
put_cfb('fileheader-twice', cfb([('FileHeader', b'HWP Document File' + b'\0' * 100), ('FileHeader', b'x' * 100), ('WordDocument', WORD)]))
put_cfb('fileheader-twice-rev', cfb([('FileHeader', b'x' * 100), ('FileHeader', b'HWP Document File' + b'\0' * 100), ('WordDocument', WORD)]))

# A summary stream at the edge of the short stream's size, and one that
# claims a size it does not have.
for n in (4095, 4096, 4097):
    data = propset([], 'Microsoft Word')
    sumcase('sum-size-%d' % n, data + b'\0' * (n - len(data)), [('Other', b'x' * 70)])
for n in (4095, 4096):
    c = cfb([('Other', b'x' * 70), SI('Microsoft Word')])
    put_cfb('sum-claim-%d' % n, c, [(c.entry_at(2) + 120, '<I', n)])

# Damage to the header, tables and directory, each on a file that is
# otherwise typed as Word.
good = cfb([('WordDocument', WORD), SI('Microsoft Word')])
put_cfb('good', good)
for tag, at, fmt, value in [('order', 0x1c, '<H', 0xfeff), ('order-zero', 0x1c, '<H', 0), ('sector-shift-8', 0x1e, '<H', 8), ('sector-shift-10', 0x1e, '<H', 10),
                            ('sector-shift-12', 0x1e, '<H', 12), ('sector-shift-20', 0x1e, '<H', 20), ('sector-shift-21', 0x1e, '<H', 21),
                            ('short-shift-5', 0x20, '<H', 5), ('short-shift-7', 0x20, '<H', 7), ('short-shift-21', 0x20, '<H', 21), ('cutoff-1000', 0x38, '<I', 1000),
                            ('cutoff-4095', 0x38, '<I', 4095), ('cutoff-4097', 0x38, '<I', 4097), ('cutoff-zero', 0x38, '<I', 0),
                            ('major-4', 0x1a, '<H', 4), ('major-zero', 0x1a, '<H', 0), ('minor', 0x18, '<H', 0x1234), ('revision', 0x18, '<H', 0x3e),
                            ('dir-sectors', 0x28, '<I', 1), ('fat-count', 0x2c, '<I', 5), ('fat-count-zero', 0x2c, '<I', 0), ('txn', 0x34, '<I', 7),
                            ('difat-count', 0x48, '<I', 1), ('difat-first', 0x44, '<I', 3), ('difat-both', 0x48, '<I', 1),
                            ('minifat-count', 0x40, '<I', 0), ('minifat-count-many', 0x40, '<I', 9), ('dir-free', 0x30, '<I', CFB_FREE),
                            ('dir-end', 0x30, '<I', CFB_END), ('dir-huge', 0x30, '<I', 500), ('dir-zero', 0x30, '<I', 0),
                            ('minifat-free', 0x3c, '<I', CFB_FREE), ('minifat-huge', 0x3c, '<I', 500), ('minifat-zero', 0x3c, '<I', 0),
                            ('minifat-end', 0x3c, '<I', CFB_END), ('fat-huge', 0x4c, '<I', 500), ('fat-free', 0x4c, '<I', CFB_FREE),
                            ('fat-second', 0x50, '<I', 1), ('fat-second-huge', 0x50, '<I', 500), ('fat-after-free', 0x54, '<I', 1),
                            ('fat-self', 0x4c, '<I', good.dir_first), ('fat-negative', 0x4c, '<I', 0x80000000), ('fat-fatsec', 0x4c, '<I', CFB_FATSEC)]:
    put_cfb('damage-%s' % tag, good, [(at, fmt, value)])
put_cfb('damage-signature', good, [(7, '<B', 0xe0)])
# A root whose first sector is the lowest negative id: no short stream.
c = cfb([('WordDocument', WORD)])
put_cfb('damage-root-start-negative', c, [(c.entry_at(0) + 116, '<I', 0x80000000)])
put_cfb('damage-signature-first', good, [(0, '<B', 0xd1)])
put_cfb('damage-clsid', good, [(8, '<B', 1)])
dirat = good.entry_at(0)
for tag, at, fmt, value in [('root-kind', dirat + 66, '<B', 2), ('root-kind-zero', dirat + 66, '<B', 0), ('root-start-free', dirat + 116, '<I', CFB_FREE),
                            ('root-start-end', dirat + 116, '<I', CFB_END), ('root-start-huge', dirat + 116, '<I', 500), ('root-size-zero', dirat + 120, '<I', 0),
                            ('root-size-huge', dirat + 120, '<I', 0x7fffffff), ('root-size-small', dirat + 120, '<I', 1),
                            ('word-start-huge', good.entry_at(1) + 116, '<I', 500), ('word-size-zero', good.entry_at(1) + 120, '<I', 0),
                            ('word-size-huge', good.entry_at(1) + 120, '<I', 0x7fffffff), ('word-size-big', good.entry_at(1) + 120, '<I', 4096),
                            ('word-start-end', good.entry_at(1) + 116, '<I', CFB_END), ('word-kind', good.entry_at(1) + 66, '<B', 1),
                            ('word-kind-root', good.entry_at(1) + 66, '<B', 5), ('word-kind-zero', good.entry_at(1) + 66, '<B', 0),
                            ('word-kind-three', good.entry_at(1) + 66, '<B', 3), ('word-kind-four', good.entry_at(1) + 66, '<B', 4),
                            ('sum-start-huge', good.entry_at(2) + 116, '<I', 500), ('sum-start-end', good.entry_at(2) + 116, '<I', CFB_END),
                            ('sum-start-free', good.entry_at(2) + 116, '<I', CFB_FREE), ('sum-size-zero', good.entry_at(2) + 120, '<I', 0),
                            ('sum-size-one', good.entry_at(2) + 120, '<I', 1), ('sum-size-huge', good.entry_at(2) + 120, '<I', 0x7fffffff),
                            ('sum-size-big', good.entry_at(2) + 120, '<I', 4096), ('sum-size-high', good.entry_at(2) + 124, '<I', 1),
                            ('sum-kind', good.entry_at(2) + 66, '<B', 1), ('sum-kind-zero', good.entry_at(2) + 66, '<B', 0),
                            ('sum-kind-five', good.entry_at(2) + 66, '<B', 5), ('second-root', good.entry_at(1) + 66, '<B', 5),
                            ('word-name-terminator', good.entry_at(1) + 24, '<H', 0x41), ('word-name-length', good.entry_at(1) + 64, '<H', 4),
                            ('word-name-length-zero', good.entry_at(1) + 64, '<H', 0)]:
    put_cfb('damage-%s' % tag, good, [(at, fmt, value)])
# Chains: the FAT's links the reads walk, cut, cycled and sent away.
fat = list(good.fat)
dir_link, minifat_link, mini_link = good.dir_first, good.minifat_first, good.mini_first
for tag, index, value in [('fat-dir-free', dir_link, CFB_FREE), ('fat-dir-self', dir_link, dir_link), ('fat-dir-huge', dir_link, 5000),
                          ('fat-dir-fatsec', dir_link, CFB_FATSEC), ('fat-dir-zero', dir_link, 0), ('fat-dir-next', dir_link, minifat_link),
                          ('fat-minifat-free', minifat_link, CFB_FREE), ('fat-minifat-self', minifat_link, minifat_link),
                          ('fat-minifat-huge', minifat_link, 5000), ('fat-minifat-next', minifat_link, mini_link),
                          ('fat-mini-free', mini_link, CFB_FREE), ('fat-mini-self', mini_link, mini_link), ('fat-mini-huge', mini_link, 5000),
                          ('fat-mini-next', mini_link, dir_link), ('fat-mini-negative', mini_link, 0x80000000)]:
    broken = list(fat)
    broken[index] = value
    data = bytearray(good.bytes(broken))
    put('cfb-damage-%s.bin' % tag, bytes(data))
# Mini FAT links of the short streams, of the summary and of the document.
for tag, value in [('self', None), ('free', CFB_FREE), ('huge', 5000), ('end', CFB_END), ('negative', 0x80000000), ('skip', 3)]:
    for who, label in ((SUMMARY, 'sum'), ('WordDocument', 'word')):
        c = cfb([('WordDocument', WORD), SI('Microsoft Word')])
        first = c.start[who]
        data = bytearray(c.bytes())
        struct.pack_into('<I', data, 512 * (c.minifat_first + 1) + 4 * first, first if value is None else value)
        put('cfb-damage-mini-%s-%s.bin' % (tag, label), bytes(data))
# A short stream's chain that runs past the short stream's end.
for who, label in ((SUMMARY, 'sum'), ('WordDocument', 'word')):
    c = cfb([('WordDocument', WORD), SI('Microsoft Word')])
    data = bytearray(c.bytes())
    table = 512 * (c.minifat_first + 1)
    struct.pack_into('<I', data, table + 4 * c.start[who], 100)
    struct.pack_into('<I', data, table + 4 * 100, CFB_END)
    put('cfb-damage-mini-beyond-%s.bin' % label, bytes(data))
# A `FileHeader` whose stream cannot be read.
c = cfb([('FileHeader', b'HWP Document File' + b'\0' * 100), ('WordDocument', WORD), SI('Microsoft Word')])
put_cfb('fileheader-broken', c, [(c.entry_at(1) + 116, '<I', 500)])
# A cycle in a chain the reads never walk, and one they do.
put_cfb('chain-word-cycle', cfb([('WordDocument', b'x' * 9000), SI('Microsoft Word')], cyclic='WordDocument'))
put_cfb('chain-word-cycle-nosummary', cfb([('WordDocument', b'x' * 9000)], cyclic='WordDocument'))
put_cfb('chain-summary-cycle', cfb([('WordDocument', WORD), (SUMMARY, propset([(7, 0x1E, lpstr('x' * 4200)[4:])], 'Microsoft Word'))], cyclic=SUMMARY))
put_cfb('chain-long', cfb([('WordDocument', b'x' * 9000), SI('Microsoft Word')]))
put_cfb('chain-long-first', cfb([('WordDocument', b'x' * 9000), SI('Microsoft Word')], data_first=True))
put_cfb('chain-summary-big-first', cfb([('WordDocument', WORD), (SUMMARY, propset([(7, 0x1E, lpstr('x' * 4200)[4:])], 'Microsoft Word'))], data_first=True))
put_cfb('chain-two-fat-sectors', cfb([('WordDocument', b'x' * 70000), SI('Microsoft Word')]))
put_cfb('chain-two-fat-sectors-first', cfb([('WordDocument', b'x' * 70000), SI('Microsoft Word')], data_first=True))
put_cfb('chain-three-fat-sectors-first', cfb([('WordDocument', b'x' * 140000), SI('Microsoft Word')], data_first=True))
put_cfb('chain-many-entries', cfb([('s%d' % i, b'x' * 70) for i in range(40)] + [('WordDocument', WORD), SI('Microsoft Word')]))
put_cfb('chain-many-entries-late', cfb([('s%d' % i, b'x' * 70) for i in range(40)] + [SI(), ('WordDocument', WORD)]))
put_cfb('chain-long-names', cfb([('s%d' % i, b'x' * 70) for i in range(40)] + [('WordDocument', WORD), SI()]))
put_cfb('chain-empty-mini', cfb([('WordDocument', b'x' * 5000)]))
put_cfb('chain-empty-mini-summary', cfb([('WordDocument', b'x' * 5000), (SUMMARY, propset([(7, 0x1E, lpstr('x' * 4200)[4:])], 'Microsoft Word'))]))
put_cfb('chain-mini-many', cfb([('a%d' % i, b'x' * 4000) for i in range(4)] + [SI('Microsoft Word')]))

# libmagic tries tar and SIMH tape before Compound File Binary: a header whose
# checksum field holds its sum (as spaces, tabs or digits lead it), and a
# record the first word opens closed by its length, even or odd, or not.
whole = bytearray(good.bytes())
total = sum(whole[:148]) + sum(whole[156:512]) + 32 * 8
for tag, field in [('tar', b'%06o\0 ' % total), ('tar-space', b' %06o\0' % total), ('tar-tab', b'\t%06o\0' % total),
                   ('tar-plain', b'%07o' % total + b'x'), ('tar-wrong', b'%06o\0 ' % (total + 1))]:
    data = bytearray(whole)
    data[148:156] = field
    put('cfb-%s.bin' % tag, bytes(data))
record = 0x11cfd0
for tag, close in [('tape', record), ('tape-odd', record - 1), ('tape-high', 0x55000000 | record), ('tape-wrong', record + 2)]:
    put('cfb-%s.bin' % tag, bytes(whole) + b'\0' * (4 + record - len(whole)) + struct.pack('<I', close))

# Cut files: every length of a small one that matters, and some of a real
# one's.
whole = good.bytes()
for n in list(range(0, 600, 60)) + list(range(512, len(whole), 512)) + [len(whole) - 1, len(whole) - 100, len(whole) - 511, len(whole) - 513, len(whole) + 1, len(whole) + 512]:
    put('cfb-cut-%05d.bin' % n, whole[:n] if n <= len(whole) else whole + b'\0' * (n - len(whole)))
for n in (8, 0x1c, 0x1e, 0x20, 0x38, 0x48, 0x4c, 0x50, 0x60, 511, 512, 513):
    put('cfb-cut-header-%03d.bin' % n, whole[:n])
for name in ('sample.doc', 'sample.xls'):
    real = open(os.path.join(corpus, name), 'rb').read()
    for n in (511, 512, 1024, 2048, len(real) - 512, len(real) - 1):
        put('cfb-cut-real-%s-%05d.bin' % (name.split('.')[1], n), real[:n])
    put('cfb-real-%s-padded.bin' % name.split('.')[1], real + b'\0' * 700)
    put('cfb-real-%s-doubled.bin' % name.split('.')[1], real + real)
