#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Compiles hs-admin's HyScribe colour scheme (ADR 0008) as core builds its
# own: crates/hyscribe-assets/colors/hyscribe/colors.scss over WordPress's
# colour Sass (wp-admin/css/colors of the oracle's image), with dart-sass,
# rtlcss and clean-css at pinned versions in a Node image pinned by digest,
# into colors.css, colors-rtl.css, colors.min.css and colors-rtl.min.css
# beside it. It first builds core's Coffee and stops unless every file
# matches the release's, so the toolchain is the one core uses.
#   reference/admin-colors/build.sh
# Needs docker, and network for the images and packages.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
IMAGE="${WORDPRESS_IMAGE:-wordpress:7.1.3-php8.4-apache}"
NODE=node@sha256:b2358485e3e33bc3a33114d2b1bdb18cdbe4df01bd2b257198eb51beb1f026c5
PACKAGES="sass@1.97.3 rtlcss@4.3.0 clean-css@5.3.3"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-assets/colors/hyscribe"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
"$DOCKER" run --rm "$IMAGE" tar -C /usr/src/wordpress/wp-admin/css -c colors | tar -C "$WORK" -x
mkdir -p "$WORK/colors/hyscribe"
cp "$OUT/colors.scss" "$WORK/colors/hyscribe/"
cp "$(dirname "$0")/compile.js" "$WORK/"
"$DOCKER" run --rm -v "$WORK:/w" -w /w "$NODE" sh -ec "
  mkdir /tmp/t && cd /tmp/t
  npm install --silent --ignore-scripts --no-audit --no-fund $PACKAGES >/dev/null
  cd /w && NODE_PATH=/tmp/t/node_modules node compile.js
"
for file in colors.css colors-rtl.css colors.min.css colors-rtl.min.css; do
  cp "$WORK/colors/hyscribe/$file" "$OUT/$file"
done
wc -c "$OUT"/*
