#!/bin/sh
# Builds Tonal.app and installs it to /Applications.
# With TONAL_NO_INSTALL=1 it only builds target/release/Tonal.app.
set -eu
cd "$(dirname "$0")/../.."

cargo build --release
app=target/release/Tonal.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/tonal "$app/Contents/MacOS/tonal"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"

iconset=$(mktemp -d)/tonal.iconset
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" ui/icon.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" ui/icon.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/tonal.icns"

# A stable certificate (from setup-signing.sh) keeps macOS permissions across
# rebuilds. Without it, ad-hoc signing changes the app's identity every build.
identity="Tonal Local Signing"
if security find-identity -p codesigning 2>/dev/null | grep -q "\"$identity\""; then
    codesign --force --sign "$identity" --identifier io.github.sjdsjadjajdjsa.tonal "$app"
else
    codesign --force --sign - "$app"
fi
if [ "${TONAL_NO_INSTALL:-}" = 1 ]; then
    echo "Built $app"
    exit 0
fi
rm -rf /Applications/Tonal.app
cp -R "$app" /Applications/
echo "Installed /Applications/Tonal.app"
