#!/bin/sh
# Builds Tonal's scene painter for the web into site/scenes.wasm. Run it after changing
# src/scenes; the built file is checked in so site/build.sh needs no Rust.
set -eu
cd "$(dirname "$0")"
rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
cargo build --release --target wasm32-unknown-unknown --target-dir ../../target/site-wasm
cp ../../target/site-wasm/wasm32-unknown-unknown/release/tonal_scenes.wasm ../scenes.wasm
ls -l ../scenes.wasm
