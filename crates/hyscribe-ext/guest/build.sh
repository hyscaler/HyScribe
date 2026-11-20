#!/bin/sh
# SPDX-License-Identifier: Apache-2.0 OR MIT
# Builds the spike's guest component from source (ADR 0022) and prints the
# path of the .wasm; the host's tests run it, and nothing built is committed.
# Needs the wasm32-wasip2 target: rustup target add wasm32-wasip2
#   build.sh [target dir]   (default: spike-guest under CARGO_TARGET_DIR or target/)
set -eu
cd "$(dirname "$0")"
dir=${1:-${CARGO_TARGET_DIR:-$(pwd)/../../../target}/spike-guest}
# The host build's flags (coverage instrumentation, a host linker) and
# wrappers are not the guest's.
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS CARGO_BUILD_RUSTFLAGS RUSTDOCFLAGS RUSTC_WORKSPACE_WRAPPER CARGO_TARGET_DIR CARGO_BUILD_TARGET
cargo build --quiet --release --target wasm32-wasip2 --locked --target-dir "$dir" >&2
echo "$dir/wasm32-wasip2/release/hyscribe_ext_spike_guest.wasm"
