#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records get_block_wrapper_attributes() from the PHP WordPress oracle
# (reference/default-install/php.sh up) into
# crates/hyscribe-render/tests/golden/wrapper-attributes.tsv, and the
# synthetic block types it uses into wrapper-types.json next to it.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
GOLDEN="$ROOT/crates/hyscribe-render/tests/golden"

"$DOCKER" cp "$ROOT/reference/supports/golden.php" "$NAME:/tmp/supports-golden.php"
"$DOCKER" exec "$NAME" php /tmp/supports-golden.php types > "$GOLDEN/wrapper-types.json"
"$DOCKER" exec "$NAME" php /tmp/supports-golden.php > "$GOLDEN/wrapper-attributes.tsv"
wc -l "$GOLDEN/wrapper-attributes.tsv"
