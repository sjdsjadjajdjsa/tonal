#!/bin/sh
# Puts mpv's official macOS build (from mpv-player/mpv's GitHub releases) inside the
# Tonal.app given as the first argument: the mpv program and its libraries in
# Contents/Helpers/mpv, its licence in Contents/Resources. Downloads are kept in
# target/macos-tools. Needs curl. For a newer mpv, change the version and checksum.
#
# The build needs macOS 15, so on macOS 14 Tonal keeps using Homebrew's mpv.
set -eu
app=$1
version=0.42.0
sha256=be1b8d2445e6880860869c080b8710dee189721f1fa4689840db6a2c10d7be69
name=mpv-v$version-macos-15-arm.zip
cache=target/macos-tools
mkdir -p "$cache"

matches() {
    [ -f "$cache/$name" ] && echo "$sha256  $cache/$name" | shasum -a 256 -c - > /dev/null 2>&1
}
if ! matches; then
    curl -fsSL -o "$cache/$name.part" "https://github.com/mpv-player/mpv/releases/download/v$version/$name"
    mv "$cache/$name.part" "$cache/$name"
    matches || { rm -f "$cache/$name"; echo "$name doesn't match its checksum" >&2; exit 1; }
fi
license=$cache/mpv-$version-LICENSE.GPL
if [ ! -s "$license" ]; then
    curl -fsSL -o "$license.part" "https://raw.githubusercontent.com/mpv-player/mpv/v$version/LICENSE.GPL"
    mv "$license.part" "$license"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
unzip -q "$cache/$name" -d "$tmp"
tar -xzf "$tmp/mpv.tar.gz" -C "$tmp"

# Only the program and its libraries (it loads them from lib/ next to itself). On its
# own it runs like Homebrew's mpv, without mpv.app's Info.plist or its mpv.conf,
# which opens a window, and macOS doesn't list it as an app.
helpers=$app/Contents/Helpers/mpv
rm -rf "$helpers"
mkdir -p "$helpers"
cp -R "$tmp/mpv.app/Contents/MacOS/." "$helpers/"
codesign --force --sign - "$helpers/mpv"

cp "$license" "$app/Contents/Resources/mpv-LICENSE.GPL.txt"
printf 'mpv %s, %s from https://github.com/mpv-player/mpv/releases/tag/v%s\n' "$version" "$name" "$version" > "$app/Contents/Resources/MPV-VERSION.txt"
echo "Bundled mpv $version"
