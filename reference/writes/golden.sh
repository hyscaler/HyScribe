#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the write oracle: for each action in reference/writes/actions.php
# and the actions-*.php files beside it,
# the rows PHP WordPress adds, changes and deletes, masked as docs/TESTING.md
# says, as crates/hyscribe-testkit/tests/golden/writes/<action>.tsv. Each
# action runs in its own PHP process inside a transaction it rolls back.
# Needs reference/default-install/php.sh up over a fresh db.sh up; take both
# down after, as the auto increment counters keep the rolled back IDs.
#   reference/writes/golden.sh [action ...]   (default: every action)
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-testkit/tests/golden/writes"
DIR=/tmp/hyscribe-writes
cleanup() { "$DOCKER" exec "$NAME" rm -rf "$DIR"; }
trap cleanup EXIT
"$DOCKER" exec "$NAME" mkdir -p "$DIR"
for path in "$ROOT"/reference/writes/*.php; do
  file="$(basename "$path")"
  "$DOCKER" exec -i "$NAME" sh -c "cat > $DIR/$file" < "$path"
done
# The front end comment posts' cases and answers (actions-comment-post.php).
"$DOCKER" exec -i "$NAME" sh -c "cat > $DIR/comment-post-cases.list" < "$ROOT/reference/comment-post/cases.list"
"$DOCKER" exec -i "$NAME" sh -c "cat > $DIR/comment-post.tsv" < "$ROOT/crates/hyscribe-http/tests/golden/comment-post.tsv"
for state in site-writes template-writes autosaves editor-saves; do
  "$DOCKER" exec -i "$NAME" sh -c "cat > $DIR/$state.sql" < "$ROOT/reference/rest/$state.sql"
done
mkdir -p "$OUT"
if [ $# -eq 0 ]; then
  set -- $("$DOCKER" exec "$NAME" php "$DIR/run.php")
fi
for action in "$@"; do
  "$DOCKER" exec "$NAME" php "$DIR/run.php" "$action" > "$OUT/$action.tsv"
  echo "$action: $(grep -vc '^#' "$OUT/$action.tsv") lines"
done
