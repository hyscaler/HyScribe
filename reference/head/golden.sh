#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the <head> PHP WordPress prints for each page of the default
# install as crates/hyscribe-render/tests/golden/head.tsv: one line per URL,
#   hex(path and query)<TAB>hex(head)
# Needs the default install oracle running (reference/default-install/db.sh
# up, then php.sh up). Page views write to the database, so start
# from a fresh db.sh up.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BASE="${HYSCRIBE_PHP_BASE:-http://127.0.0.1:8081}"
OUT="$ROOT/crates/hyscribe-render/tests/golden/head.tsv"

mkdir -p "$(dirname "$OUT")"
: > "$OUT"
for path in / '/?p=1' '/?page_id=2' '/?p=1&replytocom=1' '/?s=hello' '/?s=' '/?cat=1' '/?m=2026' '/?m=202610' \
  '/?m=20261027' '/?cat=1&m=2026' '/?p=999' '/?cat=999' '/no/such/path/'; do
  head=$(curl -s "$BASE$path" | python3 -I -c 'import sys; s=sys.stdin.buffer.read(); i=s.find(b"<head>"); j=s.find(b"</head>"); sys.stdout.write((s[i:j+7]).hex())')
  printf '%s\t%s\n' "$(printf '%s' "$path" | xxd -p | tr -d '\n')" "$head" >> "$OUT"
done
wc -l "$OUT"
