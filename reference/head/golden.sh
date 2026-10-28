#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the <head> PHP WordPress prints for each page of the default
# install as crates/hyscribe-render/tests/golden/head.tsv, and what
# wp_footer() prints after the theme's footer as footer.tsv: one line per
# URL,
#   hex(path and query)<TAB>hex(head or footer)
# Needs the default install oracle running (reference/default-install/db.sh
# up, then php.sh up). Page views write to the database, so start
# from a fresh db.sh up.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BASE="${HYSCRIBE_PHP_BASE:-http://127.0.0.1:8081}"
OUT="$ROOT/crates/hyscribe-render/tests/golden/head.tsv"
FOOTER="$ROOT/crates/hyscribe-render/tests/golden/footer.tsv"

mkdir -p "$(dirname "$OUT")"
: > "$OUT"
: > "$FOOTER"
for path in / '/?p=1' '/?page_id=2' '/?p=1&replytocom=1' '/?s=hello' '/?s=' '/?cat=1' '/?m=2026' '/?m=202610' \
  '/?m=20261027' '/?cat=1&m=2026' '/?p=999' '/?cat=999' '/no/such/path/'; do
  page=$(curl -s "$BASE$path" | xxd -p | tr -d '\n')
  head=$(printf '%s' "$page" | python3 -I -c 'import sys; s=bytes.fromhex(sys.stdin.read()); i=s.find(b"<head>"); j=s.find(b"</head>"); sys.stdout.write((s[i:j+7]).hex())')
  # The theme's footer part closes .wp-site-blocks; wp_footer() follows.
  footer=$(printf '%s' "$page" | python3 -I -c 'import sys; s=bytes.fromhex(sys.stdin.read()); m=b"</footer>\n</div>\n"; i=s.rfind(m)+len(m); j=s.rfind(b"</body>"); sys.stdout.write((s[i:j]).hex())')
  key=$(printf '%s' "$path" | xxd -p | tr -d '\n')
  printf '%s\t%s\n' "$key" "$head" >> "$OUT"
  printf '%s\t%s\n' "$key" "$footer" >> "$FOOTER"
done
wc -l "$OUT" "$FOOTER"
