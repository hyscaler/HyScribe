# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Run by reference/pages/golden.sh over the Theme Test Data: reads the page
# PHP answered from the file named by its argument and prints two lines,
# the page as hex without its RSD link, and the body field of the
# golden, the page's SHA-256.
#
# The one gallery of post 1031, `[gallery ... orderby="rand"]`, is ordered
# by RAND() in PHP, so no two runs repeat. On a page that shows it
# the field is `unshuffled:` and the SHA-256 of the page with that gallery's
# items sorted, each without its `loading="lazy"`, which PHP gives by
# position, put back where the page has it. The same on every other byte.
# crates/hyscribe-testkit/src/unshuffle.rs does the same to HyScribe's page.
import hashlib
import re
import sys

MARK = b"class='gallery galleryid-1031 "
END = b"\n\t\t</div>\n"
ITEM = re.compile(rb"<figure class='gallery-item'>.*?</figure>", re.S)
LAZY = b'<img loading="lazy" '


def unshuffle(page):
    out, at = [], 0
    while True:
        start = page.find(MARK, at)
        if start < 0:
            return b"".join(out) + page[at:]
        end = page.index(END, start)
        items = ITEM.findall(page[start:end])
        lazy = [LAZY in item for item in items]
        sorted_items = sorted(item.replace(LAZY, b"<img ", 1) for item in items)
        slots = iter(
            item.replace(b"<img ", LAZY, 1) if flag else item
            for item, flag in zip(sorted_items, lazy)
        )
        out.append(page[at:start] + ITEM.sub(lambda _: next(slots), page[start:end]))
        at = end


page = re.sub(rb"<link rel=\"EditURI\"[^\n]*?/>\n", b"", open(sys.argv[1], "rb").read())
shuffled = MARK in page
body = unshuffle(page) if shuffled else page
print(page.hex())
print(("unshuffled:" if shuffled else "") + hashlib.sha256(body).hexdigest())
