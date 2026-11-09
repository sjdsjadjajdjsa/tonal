#!/bin/sh
# Builds Tonal for Windows on Linux with cargo-xwin (the MSVC target, so it matches a
# build made on Windows) and puts tonal.exe, mpv.exe, yt-dlp.exe and the licences in
# dist/tonal. Needs clang, lld, llvm-rc, 7z and curl; the release runs it in the
# `windows` job of .gitlab-ci.yml.
set -eu
cd "$(dirname "$0")/../.."
target=x86_64-pc-windows-msvc
rustup target add "$target" > /dev/null
command -v cargo-xwin > /dev/null || cargo install --locked cargo-xwin
cargo xwin build --release --target "$target" --features bundled-sqlite
rm -rf dist
mkdir -p dist/tonal
cp "target/$target/release/tonal.exe" README.md LICENSE THIRD-PARTY-NOTICES.md dist/tonal/
cp ui/fonts/Inter-LICENSE.txt dist/tonal/FONTS-LICENSE.txt
sh packaging/windows/fetch-tools.sh dist/tonal
