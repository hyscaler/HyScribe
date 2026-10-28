#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records WordPress's escaping and sanitizing functions as the oracle for
# hyscribe-kses:
#   crates/hyscribe-kses/tests/golden/<function>.tsv
# Runs in the default install's PHP container (reference/default-install/
# php.sh up). The generator and the theme's markup are streamed into a
# temporary folder there, removed afterwards, and the goldens streamed back.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-kses/tests/golden"

mkdir -p "$OUT"
tar -C "$ROOT" -cf - reference/kses/golden.php themes/twentytwentyfive \
  | "$DOCKER" exec -i "$NAME" sh -euc '
      dir=$(mktemp -d)
      trap "rm -rf \"$dir\"" EXIT
      tar -C "$dir" -xf -
      mkdir "$dir/out"
      php -d memory_limit=1G "$dir/reference/kses/golden.php" /var/www/html "$dir/themes/twentytwentyfive" "$dir/out"
      tar -C "$dir/out" -cf - .
    ' \
  | tar -C "$OUT" -xf -
wc -l "$OUT"/*.tsv
