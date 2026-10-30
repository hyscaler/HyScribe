#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records how the main query reads the taxonomy query vars as
# crates/hyscribe-http/tests/golden/tax-query.tsv. Needs the default
# install oracle running (reference/default-install/db.sh up, then php.sh
# up); its inserts are rolled back.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/tax-query.tsv"

"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/tax-query/golden.php" > "$OUT"
wc -l "$OUT"
