#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records seeded random orders from the PHP WordPress oracle
# (reference/default-install/php.sh up) into
# crates/hyscribe-db/tests/golden/rand.tsv. Read only.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-db/tests/golden/rand.tsv"

mkdir -p "$(dirname "$OUT")"
"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/query-order/golden.php" > "$OUT"
wc -l "$OUT"
