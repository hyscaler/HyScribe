#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds the first-party classic theme components (ADR 0023) for
# wasm32-wasip2 and prints the folder holding `<slug>.wasm` for each, which
# the binary's build embeds when HYSCRIBE_THEME_COMPONENTS names it.
# scripts/package.sh runs it before the binary; tests run it to load the
# real components. Needs the wasm32-wasip2 target:
#   rustup target add wasm32-wasip2
#   scripts/themes.sh   (into themes/ under CARGO_TARGET_DIR or target/)
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
dir=${CARGO_TARGET_DIR:-$root/target}/themes
# A clean environment: a host build's flags, wrappers and coverage
# instrumentation (cargo llvm-cov, cargo-mutants, the gate script) are not the
# component's.
env -i PATH="$PATH" HOME="$HOME" TMPDIR="${TMPDIR:-/tmp}" \
  ${RUSTUP_HOME:+RUSTUP_HOME="$RUSTUP_HOME"} ${CARGO_HOME:+CARGO_HOME="$CARGO_HOME"} \
  ${RUSTUP_TOOLCHAIN:+RUSTUP_TOOLCHAIN="$RUSTUP_TOOLCHAIN"} \
  ${SOURCE_DATE_EPOCH:+SOURCE_DATE_EPOCH="$SOURCE_DATE_EPOCH"} \
  RUSTFLAGS="--remap-path-prefix=$root=/hyscribe --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo" \
  cargo build --quiet --release --locked --target wasm32-wasip2 \
  --manifest-path "$root/Cargo.toml" --target-dir "$dir/build" \
  -p hyscribe-theme-hello-elementor >&2
cp "$dir/build/wasm32-wasip2/release/hyscribe_theme_hello_elementor.wasm" "$dir/hello-elementor.wasm"
echo "$dir"
