#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records get_block_wrapper_attributes() from the PHP WordPress oracle
# (reference/default-install/php.sh up) into
# crates/hyscribe-render/tests/golden/wrapper-attributes.tsv.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/wrapper-attributes.tsv"

"$DOCKER" cp "$ROOT/reference/supports/golden.php" "$NAME:/tmp/supports-golden.php"
"$DOCKER" exec "$NAME" php /tmp/supports-golden.php > "$OUT"
wc -l "$OUT"
