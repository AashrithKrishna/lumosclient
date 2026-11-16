#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_DIR="$ROOT/dist/EmberClient.app"
CONTENTS="$APP_DIR/Contents"
MACOS="$CONTENTS/MacOS"
RESOURCES="$CONTENTS/Resources"
RES_BIN="$RESOURCES/bin"
LOGO_PNG="$ROOT/obsidian-launcher-rs/assets/emberclient_logo.png"

export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
cargo build --release --manifest-path "$ROOT/obsidian-launcher-rs/Cargo.toml"

rm -rf "$APP_DIR" "$ROOT/dist/Horizon Obsidian.app"
mkdir -p "$MACOS" "$RES_BIN" "$RESOURCES/mods"

# Build macOS .icns app icon from the griffin logo
ICONSET="$(mktemp -d)/AppIcon.iconset"
mkdir -p "$ICONSET"
for sz in 16 32 128 256 512; do
  sips -z "$sz" "$sz" "$LOGO_PNG" --out "$ICONSET/icon_${sz}x${sz}.png" >/dev/null
  sz2=$((sz * 2))
  sips -z "$sz2" "$sz2" "$LOGO_PNG" --out "$ICONSET/icon_${sz}x${sz}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$RESOURCES/AppIcon.icns"
rm -rf "$(dirname "$ICONSET")"

cat > "$CONTENTS/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>EmberClient</string>
  <key>CFBundleDisplayName</key>
  <string>EmberClient</string>
  <key>CFBundleIdentifier</key>
  <string>dev.emberclient.launcher</string>
  <key>CFBundleVersion</key>
  <string>0.2.0</string>
  <key>CFBundleShortVersionString</key>
  <string>0.2.0</string>
  <key>CFBundleExecutable</key>
  <string>emberclient-launcher</string>
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

cp "$ROOT/obsidian-launcher-rs/target/release/emberclient-launcher" "$MACOS/emberclient-launcher"
cp "$ROOT/target/release/horizon-client" "$RES_BIN/horizon-client"
cp "$ROOT/target/release/horizon-prepare" "$RES_BIN/horizon-prepare"
cp "$ROOT/research-lock.json" "$RESOURCES/research-lock.json"
cp "$ROOT/obsidian-launcher-rs/emberhud/emberhud-1.21.11.jar" "$RESOURCES/mods/emberhud-1.21.11.jar"
chmod +x "$MACOS/emberclient-launcher" "$RES_BIN/horizon-client" "$RES_BIN/horizon-prepare"

echo "Built standalone app: $APP_DIR"
ls -lh "$RESOURCES/AppIcon.icns" "$MACOS/emberclient-launcher"
