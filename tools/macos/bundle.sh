#!/bin/bash
# Build a Finder-launchable bundle before the workflow archives it.
set -euo pipefail

executable="$1"
app="$2"
root="$(cd "$(dirname "$0")/../.." && pwd)"
version="$(awk -F '"' '/^version = / { print $2; exit }' "$root/Cargo.toml")"
contents="$app/Contents"
mkdir -p "$contents/MacOS" "$contents/Resources"
cp "$executable" "$contents/MacOS/imgit"
chmod +x "$contents/MacOS/imgit"
strip -x "$contents/MacOS/imgit"

cat > "$contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>imgit</string>
    <key>CFBundleIdentifier</key><string>com.alt-orange.imgit</string>
    <key>CFBundleName</key><string>ImGit</string>
    <key>CFBundleDisplayName</key><string>ImGit</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>CFBundleIconFile</key><string>ImGit.icns</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
plutil -lint "$contents/Info.plist"

iconset="$(mktemp -d)/ImGit.iconset"
trap 'rm -rf "$(dirname "$iconset")"' EXIT
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$root/assets/images/app_icon.png" --out "$iconset/icon_${size}x${size}.png" > /dev/null
    sips -z "$((size * 2))" "$((size * 2))" "$root/assets/images/app_icon.png" --out "$iconset/icon_${size}x${size}@2x.png" > /dev/null
done
iconutil -c icns "$iconset" -o "$contents/Resources/ImGit.icns"
# Stripping invalidates the linker's signature; sign the completed bundle for Apple silicon.
codesign --force --sign - "$app"
codesign --verify --strict "$app"
