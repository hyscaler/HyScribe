#!/bin/sh
# SPDX-License-Identifier: Apache-2.0 OR MIT
# Builds the test extension, or the test theme, from source (ADR 0022, 0023)
# and prints the path of the .wasm; the host's tests run it, and nothing
# built is committed. Needs the wasm32-wasip2 target: rustup target add wasm32-wasip2
#   build.sh [theme]   (into ext-guest under CARGO_TARGET_DIR or target/)
set -eu
cd "$(dirname "$0")"
package=hyscribe_ext_guest
if [ "${1:-}" = theme ]; then package=hyscribe_ext_theme; fi
dir=${CARGO_TARGET_DIR:-$(pwd)/../../../target}/ext-guest
# A clean environment: the host build's flags, wrappers and coverage
# instrumentation (cargo llvm-cov, cargo-mutants, the gate script) are not the guest's.
env -i PATH="$PATH" HOME="$HOME" TMPDIR="${TMPDIR:-/tmp}" \
  ${RUSTUP_HOME:+RUSTUP_HOME="$RUSTUP_HOME"} ${CARGO_HOME:+CARGO_HOME="$CARGO_HOME"} \
  ${RUSTUP_TOOLCHAIN:+RUSTUP_TOOLCHAIN="$RUSTUP_TOOLCHAIN"} \
  cargo build --quiet --release --target wasm32-wasip2 --locked --target-dir "$dir" \
  -p "$(echo "$package" | tr _ -)" >&2
echo "$dir/wasm32-wasip2/release/$package.wasm"
