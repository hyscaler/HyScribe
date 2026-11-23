#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records widget group blocks from the PHP WordPress oracle
# (reference/default-install/php.sh up) into
# crates/hyscribe-render/tests/golden/widget-group.tsv. Writes nothing.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/widget-group.tsv"

"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/widget-group/golden.php" > "$OUT"
wc -l "$OUT"
