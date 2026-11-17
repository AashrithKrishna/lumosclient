#!/usr/bin/env bash
# Packages dist/Lumos-Linux-x86_64.tar.gz from Linux x86_64 programs (lumos-launcher, horizon-client,
# horizon-prepare; the "lumos release" workflow builds them on Ubuntu 22.04) plus the
# platform-independent files: Horizon's support files and compiled exporters, LumosHUD (when built)
# and authlib-injector (fetched, pinned and checksummed).
#   ./obsidian-launcher-rs/package_linux.sh <folder with the three programs>
set -euo pipefail

PROGRAMS="${1:?usage: package_linux.sh <folder with lumos-launcher, horizon-client, horizon-prepare>}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LAUNCHER="$ROOT/obsidian-launcher-rs"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$LAUNCHER/Cargo.toml" | head -1)"
AUTHLIB_VERSION="1.2.8"
AUTHLIB_SHA256="9c7f4343e6c82034958ffb48c14a2cb0c85928be7283103ce17da00c6d5a7b10"
AUTHLIB_URL="https://github.com/yushijinhun/authlib-injector/releases/download/v${AUTHLIB_VERSION}/authlib-injector-${AUTHLIB_VERSION}.jar"
AUTHLIB_JAR="$ROOT/.local/vendor/authlib-injector-${AUTHLIB_VERSION}.jar"
OUT="$ROOT/dist/Lumos-Linux-x86_64.tar.gz"

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }

for f in lumos-launcher horizon-client horizon-prepare; do
  [[ -f "$PROGRAMS/$f" ]] || { echo "error: $PROGRAMS/$f is missing" >&2; exit 1; }
done

# authlib-injector (Ely.by skins), pinned and checksummed (the same jar build_app.sh uses).
mkdir -p "$(dirname "$AUTHLIB_JAR")"
if [[ ! -f "$AUTHLIB_JAR" ]] || [[ "$(sha256 "$AUTHLIB_JAR")" != "$AUTHLIB_SHA256" ]]; then
  curl -fsSL "$AUTHLIB_URL" -o "$AUTHLIB_JAR.part"
  [[ "$(sha256 "$AUTHLIB_JAR.part")" == "$AUTHLIB_SHA256" ]] || { echo "error: authlib-injector checksum mismatch" >&2; rm -f "$AUTHLIB_JAR.part"; exit 1; }
  mv "$AUTHLIB_JAR.part" "$AUTHLIB_JAR"
fi

# Horizon's compiled exporters (Java bytecode, the same on every platform): same search as package_windows.sh.
EXPORTERS=""
if [[ -f "$ROOT/exporters/manifest.json" ]]; then
  EXPORTERS="$ROOT/exporters"
else
  for d in $(ls -td "$ROOT"/.local/exporters/*/ 2>/dev/null); do
    key="$(basename "$d")"
    if [[ -f "$d/manifest.json" ]] && grep -q "\"key\": *\"$key\"" "$d/manifest.json"; then
      EXPORTERS="${d%/}"
      break
    fi
  done
fi
[[ -n "$EXPORTERS" ]] || { echo "error: no compiled Horizon exporters found (put them in $ROOT/exporters)." >&2; exit 1; }

STAGE="$(mktemp -d)"
APP="$STAGE/Lumos"
RES="$APP/resources"
HROOT="$RES/horizon-root"
mkdir -p "$RES/bin" "$RES/mods" "$HROOT/exporters" "$HROOT/mods/horizon/src/test/java/dev/horizonlab"

install -m 755 "$PROGRAMS/lumos-launcher" "$APP/lumos-launcher"
install -m 755 "$PROGRAMS/horizon-client" "$PROGRAMS/horizon-prepare" "$RES/bin/"
cp "$LAUNCHER/assets/lumos_logo.png" "$APP/lumos.png"
# LumosHUD: one jar per supported Minecraft version (lumoshud/build_jars.sh).
hud_found=0
for jar in "$LAUNCHER"/lumoshud/lumoshud-*.jar; do
  [[ -f "$jar" ]] && { cp "$jar" "$RES/mods/"; hud_found=1; }
done
[[ $hud_found == 1 ]] || echo "warning: no lumoshud-*.jar (run lumoshud/build_jars.sh); the package won't include LumosHUD" >&2
# LumosCapes (optional until built: cd lumoscapes && ./gradlew build).
CAPES_JAR="$LAUNCHER/lumoscapes/build/libs/lumoscapes-1.21.11.jar"
[[ -f "$CAPES_JAR" ]] && cp "$CAPES_JAR" "$RES/mods/" || true
cp "$AUTHLIB_JAR" "$RES/"
cp "$ROOT/research-lock.json" "$HROOT/"
cp -R "$ROOT/mods/horizon/src/test/java/dev/horizonlab/parity" "$HROOT/mods/horizon/src/test/java/dev/horizonlab/"
cp -R "$EXPORTERS/server" "$EXPORTERS/client" "$EXPORTERS/manifest.json" "$HROOT/exporters/"

cat > "$RES/THIRD_PARTY_NOTICES.txt" <<NOTICE
Lumos Client bundles authlib-injector ${AUTHLIB_VERSION} (GNU AGPL-3.0), unmodified,
to show Ely.by skins in Java versions. Source: https://github.com/yushijinhun/authlib-injector
Horizon (GNU LGPL-3.0-or-later; older code MIT) — source: https://github.com/horizon-rust/horizon
Minecraft is not included: the launcher downloads the player's game files from Mojang.
NOTICE

# The window names itself "lumos-client" (main.rs), so desktops match it to lumos-client.desktop.
cat > "$APP/install-desktop-entry.sh" <<'INSTALL'
#!/bin/sh
# Adds Lumos Client to your applications menu (for this user). Run it again after moving this folder.
set -e
DIR="$(cd "$(dirname "$0")" && pwd)"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
mkdir -p "$APPS"
cat > "$APPS/lumos-client.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Lumos Client
Comment=Minecraft: Java Edition launcher
Exec="$DIR/lumos-launcher"
Path=$DIR
Icon=$DIR/lumos.png
Terminal=false
Categories=Game;
StartupWMClass=lumos-client
EOF
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS" >/dev/null 2>&1 || true
echo "Added Lumos Client to your applications ($APPS/lumos-client.desktop)."
INSTALL
chmod 755 "$APP/install-desktop-entry.sh"

cat > "$APP/README.txt" <<README
Lumos Client ${VERSION} for Linux (x86_64)

1. Extract this folder anywhere you like (for example ~/Applications). Keep the files together.
2. Run ./lumos-launcher, or run ./install-desktop-entry.sh once to add Lumos to your applications menu.

Lumos needs the GTK 3 and WebKitGTK 4.1 libraries, which most desktops already have:
  Debian/Ubuntu (22.04 or newer): sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0
  Fedora:                         sudo dnf install webkit2gtk4.1 gtk3
  Arch:                           sudo pacman -S webkit2gtk-4.1 gtk3
The Horizon 26.3 (Rust) engine needs Vulkan graphics drivers and ALSA (alsa-lib).

Lumos downloads Minecraft, Java and everything else it needs by itself.
Your instances and settings live in ~/LumosClient.
README

mkdir -p "$ROOT/dist"
rm -f "$OUT"
tar -C "$STAGE" -czf "$OUT" Lumos
rm -rf "$STAGE"
echo "Built Lumos Client ${VERSION} for Linux: $OUT"
ls -lh "$OUT"
