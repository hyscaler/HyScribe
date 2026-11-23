#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records featured image blocks from the PHP WordPress oracle
# (HYSCRIBE_PHP_DB=wordpress_ttd reference/default-install/php.sh up, on the
# Theme Test Data fixture) into
# crates/hyscribe-render/tests/golden/featured-image.tsv. Writes nothing.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/featured-image.tsv"
# RAW=1 prints the fixture's HTML for comparing by eye: never into the
# committed golden.
[ -n "${RAW:-}" ] && OUT=/dev/stdout

"$DOCKER" exec -i -e RAW="${RAW:-}" "$NAME" php < "$ROOT/reference/featured-image/golden.php" > "$OUT"
[ -n "${RAW:-}" ] || wc -l "$OUT"
