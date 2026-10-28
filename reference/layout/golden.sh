#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the layout support from the PHP WordPress oracle
# (reference/default-install/php.sh up) into
# crates/hyscribe-render/tests/golden/layout.tsv, and the block types it
# registers in memory into layout-types.json next to it. Read only.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/layout.tsv"

"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/layout/golden.php" > "$OUT.all"
grep '^types' "$OUT.all" | cut -f2 | xxd -r -p > "${OUT%.tsv}-types.json"
grep -v '^types' "$OUT.all" > "$OUT"
rm "$OUT.all"
wc -l "$OUT"
