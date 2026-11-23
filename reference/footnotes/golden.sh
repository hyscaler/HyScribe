#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the footnotes block from the PHP WordPress oracle
# (reference/default-install/php.sh up) into
# crates/hyscribe-render/tests/golden/footnotes.tsv. Read only.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/footnotes.tsv"

"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/footnotes/golden.php" > "$OUT"
wc -l "$OUT"
