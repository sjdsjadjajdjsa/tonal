#!/bin/sh
# Builds tonal-x86_64.AppImage in the repository root from a release build.
# yt-dlp is bundled next to the program; mpv comes from the system.
# Needs curl and file besides the Rust build tools.
set -eu
cd "$(dirname "$0")/../.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
out=tonal-x86_64.AppImage
work=target/appimage
appdir=$work/AppDir
tool=${APPIMAGETOOL:-$work/appimagetool-x86_64.AppImage}
ytdlp=$work/yt-dlp

cargo build --release --features bundled-sqlite

mkdir -p "$work"
rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" \
    "$appdir/usr/share/icons/hicolor/256x256/apps" "$appdir/usr/share/doc/tonal"
install -m 755 target/release/tonal "$appdir/usr/bin/tonal"
if [ ! -x "$ytdlp" ]; then
    curl -fsSL -o "$ytdlp" https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux
    chmod +x "$ytdlp"
fi
install -m 755 "$ytdlp" "$appdir/usr/bin/yt-dlp"
install -m 644 packaging/linux/tonal.desktop "$appdir/usr/share/applications/"
install -m 644 ui/icon.png "$appdir/usr/share/icons/hicolor/256x256/apps/tonal.png"
install -m 644 README.md LICENSE THIRD-PARTY-NOTICES.md "$appdir/usr/share/doc/tonal/"
install -m 644 ui/fonts/Inter-LICENSE.txt "$appdir/usr/share/doc/tonal/FONTS-LICENSE.txt"
ln -s usr/share/applications/tonal.desktop "$appdir/tonal.desktop"
ln -s usr/share/icons/hicolor/256x256/apps/tonal.png "$appdir/tonal.png"
ln -s tonal.png "$appdir/.DirIcon"
ln -s usr/bin/tonal "$appdir/AppRun"

if [ ! -x "$tool" ]; then
    curl -fsSL -o "$tool" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
    chmod +x "$tool"
fi

rm -f "$out" "$out.zsync"
update="gh-releases-zsync|sjdsjadjajdjsa|tonal|latest|$out.zsync"
ARCH=x86_64 VERSION="$version" APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --no-appstream -u "$update" "$appdir" "$out"
