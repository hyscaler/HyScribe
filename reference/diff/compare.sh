#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Requests the same URLs from PHP WordPress and HyScribe on the same
# database and reports where they differ (docs/TESTING.md, differential
# HTTP). Responses are kept in <out-dir> for inspection.
#   reference/diff/compare.sh <php-base> <hyscribe-base> <out-dir> [urls-file]
# The default URL list is reference/diff/default-install.urls: one path per
# line, optionally followed by the status HyScribe should give instead of
# PHP's, for an intended difference (docs/SECURITY_MODEL.md).
# Exit code: 0 when status codes and titles all match, 1 otherwise.
set -euo pipefail

php="${1:?php base URL}"
ours="${2:?hyscribe base URL}"
out="${3:?output folder}"
urls="${4:-$(dirname "$0")/default-install.urls}"
mkdir -p "$out"

title() { grep -o '<title>[^<]*</title>' "$1" | head -n 1 | sed 's/<[^>]*>//g'; }

printf '%-28s %-5s %-5s %-6s %s\n' URL PHP OURS TITLE "BYTES (php/ours)"
failed=0
n=0
while read -r path intended _; do
  [ -z "$path" ] || [ "${path#\#}" != "$path" ] && continue
  n=$((n + 1))
  for side in php ours; do
    base="$php"
    [ "$side" = ours ] && base="$ours"
    code=$(curl -s -o "$out/$n.$side.html" -w '%{http_code}' --max-redirs 0 "$base$path" || true)
    printf '%s' "$code" > "$out/$n.$side.status"
  done
  pc=$(cat "$out/$n.php.status")
  oc=$(cat "$out/$n.ours.status")
  pt=$(title "$out/$n.php.html" || true)
  ot=$(title "$out/$n.ours.html" || true)
  same_title=yes
  [ "$pt" = "$ot" ] || same_title=no
  if [ -n "${intended:-}" ] && [ "${intended#\#}" = "$intended" ]; then
    # An intended difference: only HyScribe's status is checked.
    same_title=n/a
    [ "$oc" = "$intended" ] || failed=1
  elif [ "$pc" != "$oc" ] || [ "$same_title" = no ]; then
    failed=1
  fi
  printf '%-28s %-5s %-5s %-6s %s/%s\n' "$path" "$pc" "$oc" "$same_title" \
    "$(wc -c < "$out/$n.php.html")" "$(wc -c < "$out/$n.ours.html")"
  [ "$same_title" != no ] || printf '    php:  %s\n    ours: %s\n' "$pt" "$ot"
done < "$urls"
exit "$failed"
