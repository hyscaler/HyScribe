#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Adds hs-admin's look (hyscribe-look.php, ADR 0008 amendment) to the PHP
# oracle as a must-use plugin, with the files hs-admin serves under
# wp-admin (the brand's marks), or removes it:
#   reference/admin-look/look.sh add | remove
# Every admin, login and screenshot golden script adds it before its first
# request and removes it on exit.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MU=/var/www/html/wp-content/mu-plugins
ADMIN=/var/www/html/wp-admin
# The files as hs-admin serves them, from where the binary embeds them.
FILES=(brand/hyscribe-mark.svg brand/hyscribe-mark-small.svg)
case "${1:-}" in
  add)
    # The footer names the workspace's version, as the binary does.
    version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/Cargo.toml" | head -n 1)
    sed "s/@HYSCRIBE_VERSION@/$version/" "$(dirname "$0")/hyscribe-look.php" \
      | "$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-look.php"
    tar -C "$ROOT" -c --owner=0 --group=0 \
      --transform 's,^brand/,images/,' "${FILES[@]}" \
      | "$DOCKER" exec -i "$NAME" tar -C "$ADMIN" -x --no-same-owner ;;
  remove)
    "$DOCKER" exec "$NAME" rm -rf "$MU/hyscribe-look.php" "$ADMIN/images/hyscribe-mark.svg" \
      "$ADMIN/images/hyscribe-mark-small.svg" ;;
  *) echo "usage: $0 add | remove" >&2; exit 2 ;;
esac
