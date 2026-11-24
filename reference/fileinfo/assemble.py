# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Writes the golden of golden.sh: usage `assemble.py <files> <mimes> <out>`,
# `mimes` being PHP's answers (name<TAB>mime). A synthetic file's bytes are
# in hex; those of Compound File Binary samples (`cfb-`) are mostly runs of
# one byte, written `bb{n}` for the byte bb repeated n times (n at least 8).
import os
import sys

files, mimes, out = sys.argv[1:4]


def hexed(name, data):
    if not name.startswith('cfb-'):
        return data.hex()
    parts, at = [], 0
    while at < len(data):
        run = 1
        while at + run < len(data) and data[at + run] == data[at]:
            run += 1
        parts.append('%02x{%d}' % (data[at], run) if run >= 8 else data[at:at + run].hex())
        at += run
    return ''.join(parts)


with open(mimes) as m, open(out, 'w') as o:
    for line in m:
        name, mime = line.rstrip('\n').split('\t')
        if name.startswith('corpus-'):
            o.write('corpus\t%s\t%s\n' % (name[len('corpus-'):], mime))
        else:
            data = open(os.path.join(files, name), 'rb').read()
            o.write('sample\t%s\t%s\t%s\n' % (name, hexed(name, data), mime))
