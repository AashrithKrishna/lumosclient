#!/usr/bin/env bash
# Builds dist/Lumos.app (and dist/Lumos.dmg) from a clean release build.
# Everything a player needs ships inside the app or is downloaded by the launcher itself.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LAUNCHER="$ROOT/obsidian-launcher-rs"
APP_DIR="$ROOT/dist/Lumos.app"
CONTENTS="$APP_DIR/Contents"
MACOS="$CONTENTS/MacOS"
RESOURCES="$CONTENTS/Resources"
RES_BIN="$RESOURCES/bin"
HROOT="$RESOURCES/horizon-root"
LOGO_PNG="$LAUNCHER/assets/lumos_logo.png"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$LAUNCHER/Cargo.toml" | head -1)"

AUTHLIB_VERSION="1.2.8"
AUTHLIB_SHA256="9c7f4343e6c82034958ffb48c14a2cb0c85928be7283103ce17da00c6d5a7b10"
AUTHLIB_URL="https://github.com/yushijinhun/authlib-injector/releases/download/v${AUTHLIB_VERSION}/authlib-injector-${AUTHLIB_VERSION}.jar"
VENDOR="$ROOT/.local/vendor"

export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"

# --clean: rebuild the launcher from scratch.
if [[ "${1:-}" == "--clean" ]]; then
  cargo clean --manifest-path "$LAUNCHER/Cargo.toml"
fi
cargo test --release --manifest-path "$LAUNCHER/Cargo.toml"
cargo build --release --manifest-path "$LAUNCHER/Cargo.toml"

for bin in horizon-client horizon-prepare; do
  [[ -x "$ROOT/target/release/$bin" ]] || { echo "error: $ROOT/target/release/$bin is missing; build Horizon first (cargo build --release)." >&2; exit 1; }
done

# authlib-injector (Ely.by skins), pinned and checksummed.
mkdir -p "$VENDOR"
AUTHLIB_JAR="$VENDOR/authlib-injector-${AUTHLIB_VERSION}.jar"
if [[ ! -f "$AUTHLIB_JAR" ]] || [[ "$(shasum -a 256 "$AUTHLIB_JAR" | cut -d' ' -f1)" != "$AUTHLIB_SHA256" ]]; then
  curl -fsSL "$AUTHLIB_URL" -o "$AUTHLIB_JAR.part"
  [[ "$(shasum -a 256 "$AUTHLIB_JAR.part" | cut -d' ' -f1)" == "$AUTHLIB_SHA256" ]] || { echo "error: authlib-injector checksum mismatch" >&2; rm -f "$AUTHLIB_JAR.part"; exit 1; }
  mv "$AUTHLIB_JAR.part" "$AUTHLIB_JAR"
fi

# Horizon's compiled exporters: the shipped bundle, else a freshly compiled one, else the
# cached bundle whose key matches its folder name (what compile-exporters would return).
EXPORTERS=""
if [[ -f "$ROOT/exporters/manifest.json" ]]; then
  EXPORTERS="$ROOT/exporters"
elif out="$("$ROOT/target/release/horizon-prepare" compile-exporters --root "$ROOT" 2>/dev/null | tail -1)" && [[ -f "$out/manifest.json" ]]; then
  EXPORTERS="$out"
else
  for d in $(ls -td "$ROOT"/.local/exporters/*/ 2>/dev/null); do
    key="$(basename "$d")"
    if [[ -f "$d/manifest.json" ]] && grep -q "\"key\": *\"$key\"" "$d/manifest.json"; then
      EXPORTERS="${d%/}"
      break
    fi
  done
fi
[[ -n "$EXPORTERS" ]] || { echo "error: no compiled Horizon exporters found (run horizon-prepare compile-exporters with a JDK 25)." >&2; exit 1; }

rm -rf "$APP_DIR" "$ROOT/dist/Horizon Obsidian.app" "$ROOT/dist/Lumos.dmg"
mkdir -p "$MACOS" "$RES_BIN" "$RESOURCES/mods" "$HROOT/exporters" "$HROOT/mods/horizon/src/test/java/dev/horizonlab"

# App icon from the griffin logo
ICONSET="$(mktemp -d)/AppIcon.iconset"
mkdir -p "$ICONSET"
for sz in 16 32 128 256 512; do
  sips -z "$sz" "$sz" "$LOGO_PNG" --out "$ICONSET/icon_${sz}x${sz}.png" >/dev/null
  sz2=$((sz * 2))
  sips -z "$sz2" "$sz2" "$LOGO_PNG" --out "$ICONSET/icon_${sz}x${sz}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$RESOURCES/AppIcon.icns"
rm -rf "$(dirname "$ICONSET")"

cat > "$CONTENTS/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>LumosClient</string>
  <key>CFBundleDisplayName</key>
  <string>LumosClient</string>
  <key>CFBundleIdentifier</key>
  <string>dev.lumosclient.launcher</string>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundleExecutable</key>
  <string>lumos-launcher</string>
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

cp "$LAUNCHER/target/release/lumos-launcher" "$MACOS/lumos-launcher"
cp "$ROOT/target/release/horizon-client" "$RES_BIN/horizon-client"
cp "$ROOT/target/release/horizon-prepare" "$RES_BIN/horizon-prepare"
cp "$LAUNCHER/lumoshud/lumoshud-1.21.11.jar" "$RESOURCES/mods/lumoshud-1.21.11.jar"
cp "$AUTHLIB_JAR" "$RESOURCES/authlib-injector-${AUTHLIB_VERSION}.jar"
# Horizon preparer support files (copied to ~/LumosClient/horizon on first use).
cp "$ROOT/research-lock.json" "$HROOT/research-lock.json"
cp -R "$ROOT/mods/horizon/src/test/java/dev/horizonlab/parity" "$HROOT/mods/horizon/src/test/java/dev/horizonlab/"
cp -R "$EXPORTERS/server" "$EXPORTERS/client" "$EXPORTERS/manifest.json" "$HROOT/exporters/"
cat > "$RESOURCES/THIRD_PARTY_NOTICES.txt" <<NOTICE
LumosClient bundles authlib-injector ${AUTHLIB_VERSION} (GNU AGPL-3.0), unmodified,
to show Ely.by skins in Java versions. Source: https://github.com/yushijinhun/authlib-injector
Horizon (GNU LGPL-3.0-or-later; older code MIT) — source: https://github.com/horizon-rust/horizon
Minecraft is not included: the launcher downloads the player's game files from Mojang.
NOTICE
chmod +x "$MACOS/lumos-launcher" "$RES_BIN/horizon-client" "$RES_BIN/horizon-prepare"

# Ad-hoc signature so macOS treats the bundle as one app.
codesign --force --deep --sign - "$APP_DIR" >/dev/null 2>&1 || echo "warning: ad-hoc signing failed (the app still runs locally)"

hdiutil create -volname "LumosClient" -srcfolder "$APP_DIR" -ov -format UDZO "$ROOT/dist/Lumos.dmg" >/dev/null

echo "Built LumosClient ${VERSION}: $APP_DIR"
ls -lh "$MACOS/lumos-launcher" "$ROOT/dist/Lumos.dmg"
