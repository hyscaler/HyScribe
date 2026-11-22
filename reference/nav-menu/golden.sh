#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the classic menus printed by wp_nav_menu() from the PHP WordPress oracle
# (reference/default-install/php.sh up) into
# crates/hyscribe-render/tests/golden/nav-menu.tsv. Its menus are made in a
# transaction it rolls back.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/nav-menu.tsv"

"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/nav-menu/golden.php" > "$OUT"
wc -l "$OUT"
