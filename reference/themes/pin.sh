#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Downloads the themes pinned in themes/pinned.tsv, checks each
# archive against its SHA-256 before unpacking it, and unpacks it into its own
# new folder, target/pinned-themes/<slug>, which php.sh mounts into the PHP
# oracle. Nothing from an archive is run on the host.
#   reference/themes/pin.sh [slug ...]   (default: every pinned theme)
# HYSCRIBE_THEME_ZIPS=<folder> reads <slug>.<version>.zip from there instead
# of downloading (offline use). Needs curl, unzip and sha256sum.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MANIFEST="$ROOT/themes/pinned.tsv"
OUT="$ROOT/target/pinned-themes"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

pin() {
  local slug="$1" version="$2" sha="$3"
  local name="$slug.$version.zip" zip dest
  if [ -n "${HYSCRIBE_THEME_ZIPS:-}" ]; then
    zip="$HYSCRIBE_THEME_ZIPS/$name"
  else
    zip="$WORK/$name"
    curl -fsSL --proto =https --tlsv1.2 -o "$zip" "https://downloads.wordpress.org/theme/$name"
  fi
  echo "$sha  $zip" | sha256sum --check --status || { echo "$name: SHA-256 differs from the manifest" >&2; exit 1; }
  # Only entries of the theme's own folder.
  if unzip -Z1 "$zip" | grep -qv "^$slug/"; then
    echo "$name: an entry outside $slug/" >&2
    exit 1
  fi
  dest="$OUT/$slug"
  rm -rf "$dest"
  mkdir -p "$WORK/$slug-x" "$OUT"
  # UTC, so the file times match the ones HyScribe's build reads.
  TZ=UTC unzip -q -d "$WORK/$slug-x" "$zip"
  mv "$WORK/$slug-x/$slug" "$dest"
  echo "pinned $slug $version in $dest"
}

while IFS=$'\t' read -r slug version sha; do
  case "$slug" in '' | '#'*) continue ;; esac
  if [ "$#" -eq 0 ] || printf '%s\n' "$@" | grep -qx "$slug"; then
    pin "$slug" "$version" "$sha"
  fi
done < "$MANIFEST"
