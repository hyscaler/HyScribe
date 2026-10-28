#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records block style variations as the style-variations.tsv goldens of
# hyscribe-theme-json and hyscribe-render. Needs the default
# install oracle running (reference/default-install/db.sh up, then php.sh up).
# Only reads from it.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-theme-json/tests/golden/style-variations.tsv"

"$DOCKER" cp "$ROOT/reference/theme-json/variations-golden.php" "$NAME:/tmp/variations-golden.php"
"$DOCKER" exec -i "$NAME" php /tmp/variations-golden.php < "$ROOT/reference/theme-json/variations.json" > "$OUT"
wc -l "$OUT"

RENDER="$ROOT/crates/hyscribe-render/tests/golden/style-variations.tsv"
"$DOCKER" cp "$ROOT/reference/theme-json/variations-render.php" "$NAME:/tmp/variations-render.php"
"$DOCKER" exec -i "$NAME" php /tmp/variations-render.php < "$ROOT/reference/theme-json/variations.json" > "$RENDER"
wc -l "$RENDER"
