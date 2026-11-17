#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds the release archives for one target (ADR 0005):
#   scripts/package.sh <x86_64|aarch64>-unknown-linux-musl <out-dir>
# Writes hyscribe_<version>_linux_<arch>.tar.gz and the hyscribe-latest alias.
# The build is reproducible: same commit, same toolchain, same bytes.
set -euo pipefail

target=${1:?target}
out=${2:?out-dir}
root="$(cd "$(dirname "$0")/.." && pwd)"

case "$target" in
  x86_64-unknown-linux-musl) arch=amd64 ;;
  aarch64-unknown-linux-musl)
    arch=arm64
    # No C code is linked, so Rust's own linker is enough to cross-compile.
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld
    ;;
  *) echo "unsupported target: $target" >&2; exit 2 ;;
esac

version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$root" log -1 --format=%ct)}"
# Keep hosts paths out of the binary.
export RUSTFLAGS="--remap-path-prefix=$root=/hyscribe --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"

cargo build --manifest-path "$root/Cargo.toml" --release --locked -p hyscribe --target "$target"

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -m 0755 "$stage/hyscribe"
install -m 0755 "$root/target/$target/release/hyscribe" "$stage/hyscribe/hyscribe"
install -m 0644 "$root/LICENSE" "$root/packaging/README.txt" "$root/.env.example" "$stage/hyscribe/"
# The operator documentation (MVP 9.3), as hyscribe/docs/.
install -d -m 0755 "$stage/hyscribe/docs"
install -m 0644 "$root"/docs/operators/*.md "$stage/hyscribe/docs/"

mkdir -p "$out"
archive="$out/hyscribe_${version}_linux_$arch.tar.gz"
tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
  --format=gnu -C "$stage" -cf - hyscribe | gzip -9 -n > "$archive"
cp "$archive" "$out/hyscribe-latest-linux-$arch.tar.gz"
echo "$archive"
