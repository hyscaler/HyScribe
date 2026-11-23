#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records reference/redirect/golden.php's output as
# crates/hyscribe-http/tests/golden/redirect.tsv. Needs the default install
# oracle running (db.sh up, then php.sh up); it only reads.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/redirect.tsv"
"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/redirect/golden.php" > "$OUT"
wc -l "$OUT"
