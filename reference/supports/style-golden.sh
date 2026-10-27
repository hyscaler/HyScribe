#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the style engine oracle (style-golden.php) into
# crates/hyscribe-render/tests/golden/style-engine.tsv.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/style-engine.tsv"

"$DOCKER" cp "$ROOT/reference/supports/style-golden.php" "$NAME:/tmp/style-golden.php"
"$DOCKER" exec "$NAME" php /tmp/style-golden.php > "$OUT"
wc -l "$OUT"
