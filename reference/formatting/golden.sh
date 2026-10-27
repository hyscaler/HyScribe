#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records WordPress's wptexturize() and convert_smilies() as goldens:
#   crates/hyscribe-render/tests/golden/wptexturize.tsv
#   crates/hyscribe-render/tests/golden/convert-smilies.tsv
# Needs the default install oracle running (reference/default-install/db.sh up,
# then php.sh up). Only reads from it.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden"

mkdir -p "$OUT"
"$DOCKER" exec -i "$NAME" php -- texturize < "$ROOT/reference/formatting/golden.php" > "$OUT/wptexturize.tsv"
"$DOCKER" exec -i "$NAME" php -- smilies < "$ROOT/reference/formatting/golden.php" > "$OUT/convert-smilies.tsv"
wc -l "$OUT/wptexturize.tsv" "$OUT/convert-smilies.tsv"
