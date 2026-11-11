#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records wp_filter_content_tags() over every post and page of the Theme
# Test Data fixture with an image or iframe, in and out of the loop, from
# the PHP oracle (HYSCRIBE_PHP_DB=wordpress_ttd
# reference/default-install/php.sh up) into
# crates/hyscribe-render/tests/golden/content-tags.tsv. Writes nothing.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/content-tags.tsv"

ids=$("$DOCKER" exec "$DB" mariadb -uwordpress -preference-only wordpress_ttd -N -e \
  "SELECT ID FROM wp_posts WHERE post_type IN ('post','page') AND (post_content LIKE '%<img%' OR post_content LIKE '%<iframe%') ORDER BY ID")
: > "$OUT"
for id in $ids; do
  for loop in 0 1; do
    "$DOCKER" exec -i -e POST="$id" -e LOOP="$loop" -e RAW="${RAW:-}" "$NAME" php < "$ROOT/reference/content-tags/golden.php" >> "$OUT"
  done
done
# Our own content, around the fixture's attachment 611 (canola2.jpg, 640x480).
U="http://127.0.0.1:8081/wp-content/uploads/2008/06"
cases=(
  '<iframe src="https://x.test/v" width="560" height="315"></iframe>'
  '<iframe src="x" loading="eager" width="1" height="1"></iframe><iframe src="y"></iframe><iframe width="2" height="2"></iframe>'
  "<img src=\"$U/canola2.jpg\" class=\"wp-image-611\" style=\"width: 320px;\">"
  "<img src=\"$U/canola2.jpg\" class=\"wp-image-611\" style=\"width:0px;\">"
  "<img src=\"$U/canola2-300x225.jpg?ver=1\" class=\"wp-image-611\">"
  "<img src=\"$U/canola2-300x225.jpg\" class=\"wp-image-611\" sizes=\"50vw\">"
  "<img src=\"$U/unknown.jpg\" class=\"wp-image-611\">"
  "<img src=\"$U/canola2.jpg\" class=\"WP-IMAGE-611\" width=\"640\" height=\"480\" loading=\"eager\" fetchpriority=\"low\" decoding=\"sync\">"
  "<img src=\"$U/canola2.jpg\" class=\"wp-image-611\" fetchpriority=\"high\" loading=\"lazy\">"
  "<img src=\"$U/canola2.jpg\" class=\"wp-image-611\" fetchpriority=\"high\">"
  "<img src=\"$U/canola2.jpg\" class=\"wp-image-611\" fetchpriority=\"auto\">"
  "<img	src=\"$U/canola2.jpg\" class=\"wp-image-611\">"
  "<img src='$U/canola2.jpg' class=\"wp-image-611\" width='640' height='480'>"
  '<img src="x.jpg" class="wp-image- wp-image-999999">'
  '<img src="" class="wp-image-611">'
  '<img class="wp-image-611" data-src="a">'
  "<img src=\"$U/canola2.jpg\" class=\"wp-image-611\"><p>x</p><img src=\"$U/canola2.jpg\" class=\"wp-image-611\">"
  '<img > <imgx src="a"> <img src="a.jpg" width="1" height="1"> <IMG src="b.jpg">'
  "<img src=\"$U/canola2.jpg\" class=\"wp-image-611\" width=\"640\" height=\"480\" loading=\"lazy\" sizes=\"auto, 9px\">"
  "<img src=\"$U/canola2-300x225.jpg\" class=\"wp-image-611\" width=\"300\" height=\"225\" srcset=\"a.jpg 1w\" sizes=\"9px\">"
  "<img src=\"$U/canola2.jpg\" class=\"wp-image-611\" width=\"10\" height=\"10\"><img src=\"a.jpg\" width=\"300\" height=\"300\"><img src=\"b.jpg\" width=\"300\" height=\"300\"><img src=\"c.jpg\" width=\"300\" height=\"300\">"
)
for content in "${cases[@]}"; do
  hex=$(printf '%s' "$content" | od -An -v -tx1 | tr -d ' \n')
  for loop in 0 1; do
    "$DOCKER" exec -i -e POST=1177 -e CONTENT="$hex" -e LOOP="$loop" -e RAW="${RAW:-}" "$NAME" php < "$ROOT/reference/content-tags/golden.php" >> "$OUT"
  done
done
wc -l "$OUT"
