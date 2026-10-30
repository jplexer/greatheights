#!/usr/bin/env bash
# Builds Greatheights.app in target/release/bundle, ad-hoc signed for use
# on this Mac. With --install, also replaces /Applications/Greatheights.app
# and starts it.
#
#   script/bundle-macos.sh [--install]
set -euo pipefail

cd "$(dirname "$0")/.."
name="Greatheights"
bundle_id="dev.greatheights.launcher"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
app="target/release/bundle/$name.app"

cargo build --release -p greatheights

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/greatheights "$app/Contents/MacOS/greatheights"

iconset="$(mktemp -d)/AppIcon.iconset"
swift script/make-icon.swift "$iconset"
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>$name</string>
    <key>CFBundleDisplayName</key>
    <string>$name</string>
    <key>CFBundleIdentifier</key>
    <string>$bundle_id</string>
    <key>CFBundleExecutable</key>
    <string>greatheights</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$version</string>
    <key>CFBundleVersion</key>
    <string>$version</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <!-- A background app: no Dock icon or menu bar, even while launching. -->
    <key>LSUIElement</key>
    <true/>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSAppleEventsUsageDescription</key>
    <string>Greatheights uses System Events and Finder for system commands such as Restart and Empty Trash.</string>
</dict>
</plist>
PLIST

# Ad-hoc signature: enough to run on this Mac, not to distribute.
codesign --force --sign - "$app"
codesign --verify --strict "$app"
echo "Built $app"

if [[ "${1:-}" == "--install" ]]; then
    pkill -x greatheights || true
    rm -rf "/Applications/$name.app"
    cp -R "$app" "/Applications/$name.app"
    open "/Applications/$name.app"
    echo "Installed /Applications/$name.app"
fi
