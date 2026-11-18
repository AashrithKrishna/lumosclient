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

# ── (feat/platforms) where the three programs come from ──
# Default: this Mac's own release builds (the launcher is tested and built here; Horizon must be
# built already). --prebuilt <dir>: take lumos-launcher, horizon-client and horizon-prepare from
# <dir> instead (the release workflow passes universal arm64 + x86_64 programs made with lipo).
LAUNCHER_BIN="$LAUNCHER/target/release/lumos-launcher"
ENGINE_DIR="$ROOT/target/release"
PREBUILT=""
if [[ "${1:-}" == "--prebuilt" ]]; then
  PREBUILT="$(cd "${2:?usage: build_app.sh --prebuilt <folder with lumos-launcher, horizon-client, horizon-prepare>}" && pwd)"
  LAUNCHER_BIN="$PREBUILT/lumos-launcher"
  ENGINE_DIR="$PREBUILT"
  [[ -f "$LAUNCHER_BIN" ]] || { echo "error: $LAUNCHER_BIN is missing" >&2; exit 1; }
  chmod +x "$PREBUILT"/lumos-launcher "$PREBUILT"/horizon-client "$PREBUILT"/horizon-prepare 2>/dev/null || true
else
  # --clean: rebuild the launcher from scratch.
  if [[ "${1:-}" == "--clean" ]]; then
    cargo clean --manifest-path "$LAUNCHER/Cargo.toml"
  fi
  cargo test --release --manifest-path "$LAUNCHER/Cargo.toml"
  cargo build --release --manifest-path "$LAUNCHER/Cargo.toml"
fi

for bin in horizon-client horizon-prepare; do
  [[ -x "$ENGINE_DIR/$bin" ]] || { echo "error: $ENGINE_DIR/$bin is missing; build Horizon first (cargo build --release)." >&2; exit 1; }
done
# ── end feat/platforms ──

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
elif out="$("$ENGINE_DIR/horizon-prepare" compile-exporters --root "$ROOT" 2>/dev/null | tail -1)" && [[ -f "$out/manifest.json" ]]; then
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
  <key>NSMicrophoneUsageDescription</key>
  <string>Voice chat mods in your games use the microphone.</string>
</dict>
</plist>
PLIST

cp "$LAUNCHER_BIN" "$MACOS/lumos-launcher"
cp "$ENGINE_DIR/horizon-client" "$RES_BIN/horizon-client"
cp "$ENGINE_DIR/horizon-prepare" "$RES_BIN/horizon-prepare"
# ── LumosHUD: one jar per supported Minecraft version, rebuilt with Gradle when missing or stale ──
"$LAUNCHER/lumoshud/build_jars.sh" | while read -r jar; do cp "$jar" "$RESOURCES/mods/"; done
# ── end LumosHUD ──
# LumosCapes (optional until built: cd lumoscapes && ./gradlew build).
CAPES_JAR="$LAUNCHER/lumoscapes/build/libs/lumoscapes-1.21.11.jar"
[[ -f "$CAPES_JAR" ]] && cp "$CAPES_JAR" "$RESOURCES/mods/" || echo "note: $CAPES_JAR not built; Lumos capes won't show in game."
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

# feat/updater: checksums the in-app updater verifies before installing (see RELEASING.md).
# SHA256SUMS always lists exactly the packages present in dist/, so it can't go stale.
(cd "$ROOT/dist" && shasum -a 256 Lumos.dmg > Lumos.dmg.sha256 \
  && shasum -a 256 $(ls Lumos.dmg Lumos-Windows-x64.zip 2>/dev/null) > SHA256SUMS)
# /feat/updater

echo "Built LumosClient ${VERSION}: $APP_DIR"
ls -lh "$MACOS/lumos-launcher" "$ROOT/dist/Lumos.dmg"
