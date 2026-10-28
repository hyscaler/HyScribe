#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the comment blocks from the PHP WordPress
# oracle (reference/default-install/php.sh up) into
# crates/hyscribe-render/tests/golden/comments.tsv. Its inserts are rolled back.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/comments.tsv"

"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/comments/golden.php" > "$OUT"
wc -l "$OUT"
