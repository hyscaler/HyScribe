#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Makes the site editor's saves of reference/site-edits/save.php through
# PHP WordPress and keeps the rows they wrote as
# reference/site-edits/content.sql, which reference/pages/golden.sh
# site-edits adds before recording its pages and the Rust tests load. The
# navigation menu and theme mods php.sh's first page view wrote are
# taken out first, so the saves start from the fixture's IDs. Start from a
# fresh db.sh up and php.sh up, and reset both after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/reference/site-edits/content.sql"
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only -N wordpress "$@"; }
sql <<'SQL'
DELETE FROM wp_posts WHERE ID = 4 AND post_type = 'wp_navigation';
DELETE FROM wp_options WHERE option_name = 'theme_mods_twentytwentyfive';
ALTER TABLE wp_posts AUTO_INCREMENT = 4;
SQL
posts=$(sql -e 'SELECT MAX(ID) FROM wp_posts')
meta=$(sql -e 'SELECT MAX(meta_id) FROM wp_postmeta')
terms=$(sql -e 'SELECT MAX(term_id) FROM wp_terms')
# Loading WordPress schedules its cron events, and publishing post 1 again
# clears fresh_site: neither shows on the pages.
options() { sql -e "SELECT option_name, MD5(option_value) FROM wp_options WHERE option_name NOT IN ('cron', 'fresh_site') ORDER BY option_name"; }
options=$(options)
"$DOCKER" exec -i "$NAME" php /dev/stdin < "$ROOT/reference/site-edits/save.php"
diff <(echo "$options") <(options) >&2 || { echo "the saves changed options" >&2; exit 1; }
dump() {
  "$DOCKER" exec "$DB" mariadb-dump -uroot -preference-only --compact --no-create-info --replace \
    --skip-extended-insert wordpress "$1" --where="$2" | grep '^REPLACE INTO'
}
{
  cat <<'HEAD'
-- SPDX-License-Identifier: AGPL-3.0-or-later
--
-- The rows PHP WordPress wrote for the site editor's saves of
-- reference/site-edits/save.php, as reference/site-edits/save.sh dumped
-- them: global styles, a navigation menu, edited template parts and
-- templates, a custom template given to post 1, and the sample page's
-- template. reference/pages/golden.sh site-edits adds them to the default
-- install before recording its pages.
HEAD
  dump wp_posts "ID > $posts OR ID = 1"
  dump wp_postmeta "meta_id > $meta"
  dump wp_terms "term_id > $terms"
  dump wp_term_taxonomy "term_id > $terms"
  dump wp_term_relationships "object_id > $posts"
} > "$OUT"
wc -l "$OUT"
