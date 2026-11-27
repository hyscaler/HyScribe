#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds the release archives for one target (ADR 0005):
#   scripts/package.sh <x86_64|aarch64>-unknown-linux-musl <out-dir>
#   scripts/package.sh <aarch64|x86_64>-apple-darwin <out-dir>      (on macOS)
#   scripts/package.sh x86_64-pc-windows-msvc <out-dir>             (on Windows)
# Writes hyscribe_<version>_<linux|darwin>_<arch>.tar.gz and the
# hyscribe-latest alias; on Windows hyscribe_<version>_windows_amd64.zip
# and its alias. The build is reproducible: same commit, same
# toolchains, same bytes. Needs the Rust target and clang: `ring` (rustls's
# crypto provider, ADR 0019) compiles C and assembly, and clang builds it
# for either Linux target without a musl cross toolchain, and for either
# macOS target with Xcode's command line tools. CC_<target> and AR_<target>
# override the choice. The macOS targets are built on macOS only, and need
# GNU tar (Homebrew's gnu-tar, as gtar) for an archive with the same layout
# as the Linux one. Written for bash 3.2, which is macOS's /bin/bash. The
# Windows target is built in Git for Windows's bash, with Visual Studio's
# C compiler and linker, Microsoft's C runtime linked in statically, and
# Python 3 to write the zip.
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
  x86_64-pc-windows-msvc)
    os=windows
    arch=amd64
    case "$(uname -s)" in
      MINGW* | MSYS* | CYGWIN*) ;;
      *) echo "$target is built on Windows, not cross-compiled" >&2; exit 2 ;;
    esac
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

# cc-rs reads CC_<target> with the target's dashes as underscores. On
# Windows it finds Visual Studio's compiler by itself.
var="${target//-/_}"
cc_var="CC_$var"
if [ "$os" != windows ]; then
  export "$cc_var=${!cc_var:-clang}"
  if ! command -v "${!cc_var}" > /dev/null; then
    echo "${!cc_var} not found: install clang, or set $cc_var to a C compiler for $target" >&2
    exit 2
  fi
fi

version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$root" log -1 --format=%ct)}"
# Keep host paths out of the binary.
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
if [ "$os" = windows ]; then
  # rustc sees Windows's own paths. The C runtime is linked in, so the
  # binary needs no Visual C++ redistributable, and /Brepro makes the
  # linker write a hash of the binary where it writes the build time.
  export RUSTFLAGS="--remap-path-prefix=$(cygpath -w "$root")=/hyscribe --remap-path-prefix=$(cygpath -w "$cargo_home")=/cargo -C target-feature=+crt-static -C link-arg=/Brepro"
else
  export RUSTFLAGS="--remap-path-prefix=$root=/hyscribe --remap-path-prefix=$cargo_home=/cargo"
fi

cargo build --manifest-path "$root/Cargo.toml" --release --locked -p hyscribe --target "$target"

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -m 0755 "$stage/hyscribe"
exe=hyscribe
if [ "$os" = windows ]; then exe=hyscribe.exe; fi
install -m 0755 "$root/target/$target/release/$exe" "$stage/hyscribe/$exe"
install -m 0644 "$root/LICENSE" "$root/NOTICE" "$root/packaging/README.txt" "$root/.env.example" "$stage/hyscribe/"
# The notices of what the binary embeds: WordPress, its themes and the crates.
"$root/scripts/third-party-notices.sh" "$stage/hyscribe/THIRD-PARTY-NOTICES" "$os"
chmod 0644 "$stage/hyscribe/THIRD-PARTY-NOTICES"
# The operator documentation (MVP 9.3), as hyscribe/docs/.
install -d -m 0755 "$stage/hyscribe/docs"
install -m 0644 "$root"/docs/operators/*.md "$stage/hyscribe/docs/"

mkdir -p "$out"
if [ "$os" = windows ]; then
  # A zip, as Windows opens one itself: its entries sorted by name, each
  # dated at the commit, with no system's attributes, so the same files
  # give the same bytes.
  archive="$out/hyscribe_${version}_windows_$arch.zip"
  python -I - "$stage" "$archive" "$SOURCE_DATE_EPOCH" << 'PYTHON'
import os, sys, time, zipfile
stage, archive, epoch = sys.argv[1], sys.argv[2], int(sys.argv[3])
date = time.gmtime(epoch)[:6]
names = []
for folder, dirs, files in os.walk(os.path.join(stage, "hyscribe")):
    relative = os.path.relpath(folder, stage).replace(os.sep, "/")
    names.append(relative + "/")
    names.extend(relative + "/" + name for name in files)
with zipfile.ZipFile(archive, "w") as out:
    for name in sorted(names):
        entry = zipfile.ZipInfo(name, date)
        entry.create_system = 0
        if name.endswith("/"):
            entry.external_attr = 0x10
            out.writestr(entry, b"")
        else:
            entry.compress_type = zipfile.ZIP_DEFLATED
            with open(os.path.join(stage, name), "rb") as file:
                out.writestr(entry, file.read(), compresslevel=9)
PYTHON
  cp "$archive" "$out/hyscribe-latest-$os-$arch.zip"
  echo "$archive"
  exit 0
fi

# GNU tar writes the same archive everywhere; macOS's own tar has no --sort.
tar=tar
if ! tar --version 2> /dev/null | grep -q 'GNU tar'; then
  tar=gtar
  if ! command -v gtar > /dev/null; then
    echo "GNU tar not found: install it (brew install gnu-tar)" >&2
    exit 2
  fi
fi

archive="$out/hyscribe_${version}_${os}_$arch.tar.gz"
"$tar" --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
  --format=gnu -C "$stage" -cf - hyscribe | gzip -9 -n > "$archive"
cp "$archive" "$out/hyscribe-latest-$os-$arch.tar.gz"
echo "$archive"
