#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds the release archives for one target (ADR 0005):
#   scripts/package.sh <x86_64|aarch64>-unknown-linux-musl <out-dir>
#   scripts/package.sh <aarch64|x86_64>-apple-darwin <out-dir>      (on macOS)
# Writes hyscribe_<version>_<linux|darwin>_<arch>.tar.gz and the
# hyscribe-latest alias. The build is reproducible: same commit, same
# toolchains, same bytes. Needs the Rust target and clang: `ring` (rustls's
# crypto provider, ADR 0019) compiles C and assembly, and clang builds it
# for either Linux target without a musl cross toolchain, and for either
# macOS target with Xcode's command line tools. CC_<target> and AR_<target>
# override the choice. The macOS targets are built on macOS only, and need
# GNU tar (Homebrew's gnu-tar, as gtar) for an archive with the same layout
# as the Linux one. Written for bash 3.2, which is macOS's /bin/bash.
set -euo pipefail

target=${1:?target}
out=${2:?out-dir}
root="$(cd "$(dirname "$0")/.." && pwd)"

os=linux
case "$target" in
  x86_64-unknown-linux-musl) arch=amd64 ;;
  aarch64-apple-darwin | x86_64-apple-darwin)
    os=darwin
    if [ "$target" = x86_64-apple-darwin ]; then arch=amd64; else arch=arm64; fi
    if [ "$(uname -s)" != Darwin ]; then
      echo "$target is built on macOS, not cross-compiled" >&2
      exit 2
    fi
    # The oldest macOS the binary starts on, whatever the building machine's
    # SDK defaults to: 11.0 is the first release for Apple Silicon.
    export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"
    ;;
  aarch64-unknown-linux-musl)
    arch=arm64
    # ring's objects are static and need no C library, so Rust's own linker
    # cross-links them with Rust's bundled musl.
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld
    # wasmtime's C helpers include the C library's headers: musl's own for
    # arm64 (Debian's musl-dev:arm64), not the host's glibc ones.
    musl=/usr/include/aarch64-linux-musl
    if [ -z "${CFLAGS_aarch64_unknown_linux_musl:-}" ] && [ -d "$musl" ]; then
      export CFLAGS_aarch64_unknown_linux_musl="-nostdinc -isystem $musl -isystem $(clang -print-resource-dir)/include"
    fi
    ;;
  *) echo "unsupported target: $target" >&2; exit 2 ;;
esac

# cc-rs reads CC_<target> with the target's dashes as underscores.
var="${target//-/_}"
cc_var="CC_$var"
export "$cc_var=${!cc_var:-clang}"
if ! command -v "${!cc_var}" > /dev/null; then
  echo "${!cc_var} not found: install clang, or set $cc_var to a C compiler for $target" >&2
  exit 2
fi

version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$root" log -1 --format=%ct)}"
# Keep host paths out of the binary.
export RUSTFLAGS="--remap-path-prefix=$root=/hyscribe --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"

cargo build --manifest-path "$root/Cargo.toml" --release --locked -p hyscribe --target "$target"

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -m 0755 "$stage/hyscribe"
install -m 0755 "$root/target/$target/release/hyscribe" "$stage/hyscribe/hyscribe"
install -m 0644 "$root/LICENSE" "$root/NOTICE" "$root/packaging/README.txt" "$root/.env.example" "$stage/hyscribe/"
# The notices of what the binary embeds: WordPress, its themes and the crates.
"$root/scripts/third-party-notices.sh" "$stage/hyscribe/THIRD-PARTY-NOTICES" "$os"
chmod 0644 "$stage/hyscribe/THIRD-PARTY-NOTICES"
# The operator documentation (MVP 9.3), as hyscribe/docs/.
install -d -m 0755 "$stage/hyscribe/docs"
install -m 0644 "$root"/docs/operators/*.md "$stage/hyscribe/docs/"

# GNU tar writes the same archive everywhere; macOS's own tar has no --sort.
tar=tar
if ! tar --version 2> /dev/null | grep -q 'GNU tar'; then
  tar=gtar
  if ! command -v gtar > /dev/null; then
    echo "GNU tar not found: install it (brew install gnu-tar)" >&2
    exit 2
  fi
fi

mkdir -p "$out"
archive="$out/hyscribe_${version}_${os}_$arch.tar.gz"
"$tar" --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
  --format=gnu -C "$stage" -cf - hyscribe | gzip -9 -n > "$archive"
cp "$archive" "$out/hyscribe-latest-$os-$arch.tar.gz"
echo "$archive"
