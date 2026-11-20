#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds the page lists of a bundled theme from the default theme's:
#   reference/pages/theme-list.sh <theme slug> <short name>
# writes default-<short>.list and ttd-<short>.list, each line of default.list
# and ttd.list with the options `template` and `stylesheet` set to the theme
# first (reference/pages/options.php serves the request with that theme
# active). reference/pages/golden.sh records them. Only reads and writes
# those two files.
set -euo pipefail

SLUG="${1:?usage: $0 <theme slug> <short name>}"
SHORT="${2:?usage: $0 <theme slug> <short name>}"
DIR="$(cd "$(dirname "$0")" && pwd)"
for base in default ttd; do
  awk -F '\t' -v OFS='\t' -v theme="template=$SLUG&stylesheet=$SLUG" \
    '{ $1 = ($1 == "") ? theme : theme "&" $1; print }' "$DIR/$base.list" > "$DIR/$base-$SHORT.list"
done
wc -l "$DIR/default-$SHORT.list" "$DIR/ttd-$SHORT.list"
