#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records how Apache, in front of PHP WordPress, answers request lines at and
# past its LimitRequestLine (8190 by default) as
# crates/hyscribe-http/tests/golden/request-line.tsv:
#   method<TAB>version<TAB>request line length<TAB>status<TAB>content type<TAB>hex(body)
# The length counts the method, URL and protocol with their spaces, not the
# line's CRLF. A body only matters for a 414, and it is recorded without its
# <hr> and <address> lines, which name Apache and the host. Needs the default
# install oracle running (reference/default-install/db.sh up, then php.sh up).
set -euo pipefail

BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/request-line.tsv"
BODY=$(mktemp)
trap 'rm -f "$BODY"' EXIT

: > "$OUT"
for method in GET HEAD OPTIONS; do
  for version in 1.0 1.1; do
    for length in 8191 8192 20000; do
      # METHOD SP /?q=aaa... SP HTTP/x.y
      filler=$((length - ${#method} - 1 - 4 - 1 - 8))
      path="/?q=$(head -c "$filler" /dev/zero | tr '\0' a)"
      case "$method" in
        HEAD) request=(--head) ;;
        *) request=(--request "$method") ;;
      esac
      meta=$(curl -s "${request[@]}" "--http$version" -o "$BODY" \
        -w '%{http_code}\t%{content_type}' "$BASE$path")
      status=${meta%%$'\t'*}
      body=""
      # A HEAD reply has no body; curl writes its headers there instead.
      if [ "$status" = 414 ] && [ "$method" != HEAD ]; then
        body=$(sed -e '/^<hr>$/d' -e '/^<address>/d' "$BODY" | xxd -p | tr -d '\n')
      fi
      printf '%s\t%s\t%s\t%s\t%s\n' "$method" "$version" "$length" "$meta" "$body" >> "$OUT"
    done
  done
done
echo "wrote $(wc -l < "$OUT") request lines to $OUT"
