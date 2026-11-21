#!/bin/sh
set -eu

cd "$(dirname "$0")/.."
export MACOSX_DEPLOYMENT_TARGET=14.0
cargo build --locked --release

target_dir=${CARGO_TARGET_DIR:-target}
architecture=$(uname -m)
app="dist/macos-$architecture/Seyir.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m 755 "$target_dir/release/seyir" "$app/Contents/MacOS/Seyir"
install -m 644 packaging/macos/Info.plist "$app/Contents/Info.plist"
install -m 644 packaging/macos/Seyir.icns "$app/Contents/Resources/Seyir.icns"
plutil -lint "$app/Contents/Info.plist"
codesign --force --sign - "$app"
codesign --verify --strict "$app"
version=$(plutil -extract CFBundleShortVersionString raw "$app/Contents/Info.plist")
ditto -c -k --sequesterRsrc --keepParent "$app" "dist/Seyir-$version-macos-$architecture.zip"
sh scripts/bundle-macos-dmg.sh "$app" "dist/Seyir-$version-macos-$architecture.dmg"
