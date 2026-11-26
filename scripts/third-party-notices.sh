#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Writes THIRD-PARTY-NOTICES for the release archive (scripts/package.sh):
#   scripts/third-party-notices.sh <out-file> [linux|darwin]
# Part 1 is WordPress and the themes the binary embeds: where their source is
# (the versions and SHA-256 values the build pins), each theme's copyright
# section, its fonts' licence files and WordPress's license.txt (a copy in
# packaging/, named for the version wordpress/VERSION pins). Part 2 is
# every crate the hyscribe binary links on the release targets of that
# system, Linux by default (its normal dependencies, from Cargo.lock, which
# differ between the two), with the licence files each crate ships,
# identical texts printed once. Its inputs are the repository and the
# checksummed crate sources of Cargo.lock, so the output is the same on
# every machine. A crate that ships no licence file takes its licence text
# from packaging/licenses/<expression>.txt; with none there, the script fails.
# Needs cargo, jq and sha256sum (or shasum, as macOS has it). It keeps to
# what GNU and BSD tools share and to bash 3.2, so it also runs on macOS.
set -euo pipefail
export LC_ALL=C

out=${1:?out-file}
root="$(cd "$(dirname "$0")/.." && pwd)"
case "${2:-linux}" in
  linux)
    targets=(x86_64-unknown-linux-musl aarch64-unknown-linux-musl)
    library="library and, on these musl targets, the musl C library into it; their"
    ;;
  darwin)
    targets=(aarch64-apple-darwin x86_64-apple-darwin)
    library="library into it, and none of the system's own libraries; its"
    ;;
  *) echo "usage: $0 <out-file> [linux|darwin]" >&2; exit 2 ;;
esac
repo=https://github.com/hyscaler/HyScribe

wp_version="$(tr -d '[:space:]' < "$root/wordpress/VERSION")"
wp_sha="$(sed -n 's/^const SHA256: &str = "\([0-9a-f]*\)";$/\1/p' "$root/crates/hyscribe-assets/build.rs")"
test -n "$wp_sha"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

if command -v sha256sum > /dev/null; then
  sha256() { sha256sum; }
else
  sha256() { shasum -a 256; }
fi
# The files under a folder, as paths inside it (find's -printf is GNU only).
files_in() { (cd "$1" && shift && find . -type f "$@") | sed 's|^\./||' | sort; }

rule() { printf '%s\n' "$1" "$(printf '%*s' "${#1}" '' | tr ' ' "$2")"; }

{
  rule "HyScribe third-party notices" "="
  echo
  echo "HyScribe is licensed under the GNU Affero General Public License, version 3"
  echo "or later (see LICENSE). The hyscribe binary also contains the works below,"
  echo "each under its own licence."
  echo
  echo
  rule "Part 1: WordPress and its themes" "="
  echo
  echo "The binary embeds the files of wp-includes and wp-admin of WordPress"
  echo "$wp_version and the files of the themes listed below, all but their PHP"
  echo "files (and, of a script or stylesheet WordPress also ships minified,"
  echo "mostly only the minified copy). They are served unmodified at their usual"
  echo "URLs. The embedded files keep their own copyright and licence comments,"
  echo "and the licence files WordPress ships beside them (for example"
  echo "wp-includes/js/tinymce/license.txt and wp-includes/js/plupload/license.txt)"
  echo "are embedded and served with them."
  echo
  rule "Corresponding Source" "-"
  echo
  echo "The source of the embedded WordPress files is the WordPress $wp_version release:"
  echo
  echo "  https://wordpress.org/wordpress-$wp_version.tar.gz"
  echo "  SHA-256 $wp_sha"
  echo
  echo "The build downloads this archive and checks its SHA-256 before using it."
  echo "Each theme comes from that archive or from its pinned wordpress.org archive:"
  echo
  for dir in "$root"/themes/*/; do
    slug="$(basename "$dir")"
    pin="$(awk -F'\t' -v s="$slug" '$1 == s { print $2 "\t" $3 }' "$root/themes/pinned.tsv")"
    if [ -n "$pin" ]; then
      echo "  $slug: https://downloads.wordpress.org/theme/$slug.${pin%%$'\t'*}.zip"
      echo "    SHA-256 ${pin#*$'\t'}"
    else
      echo "  $slug: wordpress/wp-content/themes/$slug in the WordPress $wp_version archive"
    fi
  done
  echo
  echo "HyScribe also keeps each theme's non-PHP files, with its PHP patterns"
  echo "rendered to block markup, in the themes/ folder of its own source:"
  echo "$repo"
  for dir in "$root"/themes/*/; do
    slug="$(basename "$dir")"
    echo
    rule "Theme $slug" "-"
    echo
    tr -d '\r' < "$dir/readme.txt" | sed -n '/^== Copyright ==$/,$p'
    files_in "$dir" \( -iname 'license*' -o -iname 'copying*' \) |
      while read -r file; do
        echo
        rule "themes/$slug/$file" "."
        echo
        tr -d '\r' < "$dir/$file"
      done
  done
  echo
  rule "WordPress license.txt" "-"
  echo
  tr -d '\r' < "$root/packaging/wordpress-$wp_version-license.txt"
  echo
  echo
  rule "Part 2: Rust crates" "="
  echo
  echo "The binary is built with the Rust toolchain, which links Rust's standard"
  echo "$library"
  echo "notices are in the toolchain's share/doc/rust/COPYRIGHT-library.html."
  echo "The crates it links follow, with the licence each declares, then the"
  echo "licence files they ship."
} > "$work/part1"

# Every package, then the walk from hyscribe over normal dependencies only:
# build scripts' and tests' dependencies are not in the binary.
filters=()
for t in "${targets[@]}"; do filters+=(--filter-platform "$t"); done
cargo metadata --manifest-path "$root/Cargo.toml" --format-version 1 --locked "${filters[@]}" > "$work/meta.json"
jq -r '
  (.resolve.nodes | map({key: .id, value: [.deps[] | select(any(.dep_kinds[]; .kind == null)) | .pkg]}) | from_entries) as $g
  | (.packages | map(select(.name == "hyscribe" and .source == null)) | .[0].id) as $root
  | ({seen: {($root): true}, todo: [$root]}
     | until(.todo == []; .todo[0] as $n | .todo |= .[1:]
         | reduce $g[$n][] as $d (.; if .seen[$d] then . else .seen[$d] = true | .todo += [$d] end))
     | .seen) as $linked
  | .packages[] | select($linked[.id] and .source != null)
  | [.name, .version, (.license // "none declared"), (.license_file // "-"),(.manifest_path | rtrimstr("/Cargo.toml"))]
  | @tsv' "$work/meta.json" | sort -t$'\t' -k1,1 -k2,2V > "$work/crates.tsv"

: > "$work/texts.tsv"
# No field is empty: read would merge two tabs into one.
while IFS=$'\t' read -r name version license license_file dir; do
  files=()
  while read -r entry; do
    if [ -d "$dir/$entry" ]; then
      while read -r f; do files+=("$entry/$f"); done < <(files_in "$dir/$entry")
    else
      files+=("$entry")
    fi
  done < <(ls -A "$dir" | grep -iE '^(licen[cs]e|copying|copyright|notice|unlicense)' | sort)
  if [ "$license_file" != - ] && [ -f "$dir/$license_file" ] && ! printf '%s\n' "${files[@]+"${files[@]}"}" | grep -qxF "$license_file"; then
    files+=("$license_file")
  fi
  if [ ${#files[@]} -eq 0 ]; then
    fallback="$root/packaging/licenses/${license// /-}.txt"
    if [ ! -f "$fallback" ]; then
      echo "$name $version ships no licence file and $fallback is missing" >&2
      exit 1
    fi
    printf '%s\t%s\t%s\t%s\n' "$(tr -d '\r' < "$fallback" | sha256 | cut -c1-64)" "$name $version" "LICENSE" "$fallback" >> "$work/texts.tsv"
  else
    for f in "${files[@]}"; do
      printf '%s\t%s\t%s\t%s\n' "$(tr -d '\r' < "$dir/$f" | sha256 | cut -c1-64)" "$name $version" "$f" "$dir/$f" >> "$work/texts.tsv"
    done
  fi
done < "$work/crates.tsv"

{
  cat "$work/part1"
  echo
  awk -F'\t' '{ printf "  %s %s: %s\n", $1, $2, $3 }' "$work/crates.tsv"
  # Each distinct text once, in the order of the first crate that ships it.
  awk -F'\t' '!seen[$1]++ { print $1 "\t" $3 "\t" $4 }' "$work/texts.tsv" |
    while IFS=$'\t' read -r hash file path; do
      users="$(awk -F'\t' -v h="$hash" '$1 == h { print $2 }' "$work/texts.tsv" | awk '!seen[$0]++' | paste -sd, - | sed 's/,/, /g')"
      echo
      rule "$file" "-"
      echo
      echo "Shipped by: $users" | fold -s -w 78 | sed 's/ *$//'
      echo
      tr -d '\r' < "$path"
    done
} > "$out"
