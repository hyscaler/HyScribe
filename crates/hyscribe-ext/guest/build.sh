#!/bin/sh
# SPDX-License-Identifier: Apache-2.0 OR MIT
# Rebuilds the spike's guest component into ../tests/spike.wasm (needs the
# wasm32-wasip2 target: rustup target add wasm32-wasip2).
set -eu
cd "$(dirname "$0")"
export CARGO_TARGET_DIR=../../../target/spike-guest
cargo build --release --target wasm32-wasip2 --locked
cp "$CARGO_TARGET_DIR"/wasm32-wasip2/release/hyscribe_ext_spike_guest.wasm ../tests/spike.wasm
