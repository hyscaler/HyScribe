#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the comment blocks from the PHP WordPress
# oracle (reference/default-install/php.sh up) into
# crates/hyscribe-render/tests/golden/comments.tsv, and as the author of a
# held comment previews them into held-comments.tsv, with the commenter
# cookies too, and records what sanitize_comment_cookies() leaves of cookie
# values in crates/hyscribe-write/tests/golden/comment-cookies.tsv. Its
# inserts are rolled back.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/comments.tsv"

"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/comments/golden.php" > "$OUT"
wc -l "$OUT"
"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/comments/held-golden.php" > "${OUT%/*}/held-comments.tsv"
wc -l "${OUT%/*}/held-comments.tsv"
"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/comments/cookies-golden.php" > "$ROOT/crates/hyscribe-write/tests/golden/comment-cookies.tsv"
wc -l "$ROOT/crates/hyscribe-write/tests/golden/comment-cookies.tsv"
