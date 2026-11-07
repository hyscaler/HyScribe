#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the rewrite rules WordPress generates for a range of permalink
# settings, as crates/hyscribe-http/tests/golden/rewrite-rules.tsv:
#   setup<TAB>structure<TAB>category base<TAB>tag base<TAB>page on front
# followed by that setup's rules, one regex<TAB>query per line.
# Needs the default install oracle running (db.sh up, then php.sh up). Only
# reads from it: the settings come from option filters.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/rewrite-rules.tsv"

: > "$OUT"
while IFS='|' read -r structure category tag front; do
  printf 'setup\t%s\t%s\t%s\t%s\n' "$structure" "$category" "$tag" "$front" >> "$OUT"
  "$DOCKER" exec -i "$NAME" php /dev/stdin "$structure" "$category" "$tag" "$front" < "$ROOT/reference/rewrite/rules-golden.php" >> "$OUT"
done <<'SETUPS'
/%year%/%monthnum%/%day%/%postname%/|||
/%year%/%monthnum%/%postname%/|||
/%postname%/|||
/%postname%/|||2
/archives/%post_id%|||
/index.php/%year%/%monthnum%/%day%/%postname%/|||
/index.php/%postname%/|topics||
/%category%/%postname%/|||
/%author%/%postname%/|||
/blog/%postname%/|topics|/labels|
/blog/%year%/%postname%.html|/sections/|tags|2
/%post_id%/|||
/%day%/%monthnum%/%year%/%postname%|||
/%monthnum%/%day%/%year%/%post_id%/|||
/%year%/%monthnum%/%day%/%hour%/%minute%/%second%/|||
/%pagename%/|||
SETUPS
wc -l "$OUT"
