#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records wp_text_diff()'s tables for reference/text-diff/golden.php's text
# pairs, in the oracle's WordPress image (its PHP and PCRE), as
# crates/hyscribe-render/tests/golden/text-diff.tsv. Needs no database.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
IMAGE="${WORDPRESS_IMAGE:-wordpress:7.1.3-php8.4-apache}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/text-diff.tsv"
"$DOCKER" run --rm -i "$IMAGE" php /dev/stdin < "$ROOT/reference/text-diff/golden.php" > "$OUT"
wc -l "$OUT"
