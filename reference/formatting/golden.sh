#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records WordPress's wptexturize(), convert_smilies(), force_balance_tags(),
# wpautop(), make_clickable(), remove_accents(), sanitize_text_field(),
# sanitize_user() (and strict) and the admin's display filters of user names and emails as
# goldens in crates/hyscribe-render/tests/golden.
# Needs the default install oracle running (reference/default-install/db.sh up,
# then php.sh up). Only reads from it.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden"

mkdir -p "$OUT"
"$DOCKER" exec -i "$NAME" php -- texturize < "$ROOT/reference/formatting/golden.php" > "$OUT/wptexturize.tsv"
"$DOCKER" exec -i "$NAME" php -- smilies < "$ROOT/reference/formatting/golden.php" > "$OUT/convert-smilies.tsv"
"$DOCKER" exec -i "$NAME" php -- balance < "$ROOT/reference/formatting/golden.php" > "$OUT/force-balance-tags.tsv"
"$DOCKER" exec -i "$NAME" php -- autop < "$ROOT/reference/formatting/golden.php" > "$OUT/wpautop.tsv"
"$DOCKER" exec -i "$NAME" php -- clickable < "$ROOT/reference/formatting/golden.php" > "$OUT/make-clickable.tsv"
"$DOCKER" exec -i "$NAME" php -- accents < "$ROOT/reference/formatting/golden.php" > "$OUT/remove-accents.tsv"
"$DOCKER" exec -i "$NAME" php -- text-field < "$ROOT/reference/formatting/golden.php" > "$OUT/sanitize-text-field.tsv"
"$DOCKER" exec -i "$NAME" php -- user < "$ROOT/reference/formatting/golden.php" > "$OUT/sanitize-user.tsv" "$OUT/user-display.tsv"
"$DOCKER" exec -i "$NAME" php -- user-display < "$ROOT/reference/formatting/golden.php" > "$OUT/user-display.tsv"
wc -l "$OUT/wptexturize.tsv" "$OUT/convert-smilies.tsv" "$OUT/force-balance-tags.tsv" "$OUT/wpautop.tsv" "$OUT/make-clickable.tsv" "$OUT/remove-accents.tsv" "$OUT/sanitize-text-field.tsv" "$OUT/sanitize-user.tsv" "$OUT/user-display.tsv"
