#!/usr/bin/env bash
# Packages dist/Lumos-Windows-x64.zip from the Windows programs that the "lumos windows" GitHub
# workflow builds (lumos-launcher.exe, horizon-client.exe, horizon-prepare.exe) plus the
# platform-independent files this checkout already has (Horizon support files, LumosHUD, authlib-injector).
#   ./obsidian-launcher-rs/package_windows.sh <folder with the three .exe files>
set -euo pipefail

PROGRAMS="${1:?usage: package_windows.sh <folder with lumos-launcher.exe, horizon-client.exe, horizon-prepare.exe>}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LAUNCHER="$ROOT/obsidian-launcher-rs"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$LAUNCHER/Cargo.toml" | head -1)"
AUTHLIB_VERSION="1.2.8"
AUTHLIB_SHA256="9c7f4343e6c82034958ffb48c14a2cb0c85928be7283103ce17da00c6d5a7b10"
AUTHLIB_JAR="$ROOT/.local/vendor/authlib-injector-${AUTHLIB_VERSION}.jar"
HUD_JAR="$LAUNCHER/lumoshud/lumoshud-1.21.11.jar"

for f in lumos-launcher.exe horizon-client.exe horizon-prepare.exe; do
  [[ -f "$PROGRAMS/$f" ]] || { echo "error: $PROGRAMS/$f is missing" >&2; exit 1; }
done
[[ -f "$HUD_JAR" ]] || { echo "error: $HUD_JAR is missing (run lumoshud/build_hud.sh)" >&2; exit 1; }
[[ -f "$AUTHLIB_JAR" && "$(shasum -a 256 "$AUTHLIB_JAR" | cut -d' ' -f1)" == "$AUTHLIB_SHA256" ]] \
  || { echo "error: run build_app.sh once to fetch authlib-injector ${AUTHLIB_VERSION}" >&2; exit 1; }

# Horizon's compiled exporters (Java bytecode, the same on every platform): same search as build_app.sh.
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
[[ -n "$EXPORTERS" ]] || { echo "error: no compiled Horizon exporters found (run build_app.sh first)." >&2; exit 1; }

STAGE="$(mktemp -d)"
APP="$STAGE/Lumos"
RES="$APP/resources"
HROOT="$RES/horizon-root"
mkdir -p "$RES/bin" "$RES/mods" "$HROOT/exporters" "$HROOT/mods/horizon/src/test/java/dev/horizonlab"

cp "$PROGRAMS/lumos-launcher.exe" "$APP/Lumos.exe"
cp "$PROGRAMS/horizon-client.exe" "$PROGRAMS/horizon-prepare.exe" "$RES/bin/"
cp "$HUD_JAR" "$RES/mods/"
cp "$AUTHLIB_JAR" "$RES/"
cp "$ROOT/research-lock.json" "$HROOT/"
cp -R "$ROOT/mods/horizon/src/test/java/dev/horizonlab/parity" "$HROOT/mods/horizon/src/test/java/dev/horizonlab/"
cp -R "$EXPORTERS/server" "$EXPORTERS/client" "$EXPORTERS/manifest.json" "$HROOT/exporters/"

cat > "$RES/THIRD_PARTY_NOTICES.txt" <<NOTICE
Lumos Client bundles authlib-injector ${AUTHLIB_VERSION} (GNU AGPL-3.0), unmodified,
to show Ely.by skins in Java versions. Source: https://github.com/yushijinhun/authlib-injector
Horizon (MIT) — https://github.com/horizon-rust/horizon
Minecraft is not included: the launcher downloads the player's game files from Mojang.
NOTICE

cat > "$APP/README.txt" <<README
Lumos Client ${VERSION} for Windows 10 and 11 (64-bit)

1. Unzip this folder anywhere (for example your Desktop or Documents). Keep the files together.
2. Double-click Lumos.exe.
   If Windows shows "Windows protected your PC", click "More info", then "Run anyway".
   (The app isn't signed with a paid certificate yet.)

Lumos downloads Minecraft, Java and everything else it needs by itself.
Your instances and settings live in %USERPROFILE%\\LumosClient.
README

rm -f "$ROOT/dist/Lumos-Windows-x64.zip"
mkdir -p "$ROOT/dist"
(cd "$STAGE" && zip -qr -X "$ROOT/dist/Lumos-Windows-x64.zip" Lumos)
rm -rf "$STAGE"
echo "Built Lumos Client ${VERSION} for Windows: $ROOT/dist/Lumos-Windows-x64.zip"
ls -lh "$ROOT/dist/Lumos-Windows-x64.zip"
