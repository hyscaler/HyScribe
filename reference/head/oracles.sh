#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the script module, font face, relative CSS link and <style> text
# oracles behind hyscribe-render's wp_head() as
#   crates/hyscribe-render/tests/golden/script-modules.tsv
#   crates/hyscribe-render/tests/golden/fonts-css.tsv
# Needs the default install oracle running (reference/default-install/db.sh
# up, then php.sh up). Only reads from it.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden"

"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/head/modules.php" > "$OUT/script-modules.tsv" 2>/dev/null
"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/head/fonts.php" > "$OUT/fonts-css.tsv" 2>/dev/null
wc -l "$OUT/script-modules.tsv" "$OUT/fonts-css.tsv"
