#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
MC_DIR="$HOME/Library/Application Support/minecraft"
VER_JAR="$MC_DIR/versions/Fabric 1.21.11/Fabric 1.21.11.jar"
LIBS="$MC_DIR/libraries"
JAVAC="/Library/Java/JavaVirtualMachines/jdk-21.jdk/Contents/Home/bin/javac"
JAR_BIN="/Library/Java/JavaVirtualMachines/jdk-21.jdk/Contents/Home/bin/jar"

CP="$VER_JAR"
CP="$CP:$LIBS/net/fabricmc/sponge-mixin/0.17.2+mixin.0.8.7/sponge-mixin-0.17.2+mixin.0.8.7.jar"
CP="$CP:$(find "$LIBS/org/lwjgl" -name '*.jar' | tr '\n' ':')"

BUILD="$DIR/build"
rm -rf "$BUILD"
mkdir -p "$BUILD/classes"

"$JAVAC" -proc:none --release 21 -cp "$CP" -d "$BUILD/classes" \
  "$DIR/src/dev/emberclient/hud/EmberHudMixin.java" \
  "$DIR/src/dev/emberclient/hud/EmberHudRenderer.java"

cp "$DIR/resources/fabric.mod.json" "$BUILD/classes/"
cp "$DIR/resources/emberhud.mixins.json" "$BUILD/classes/"

"$JAR_BIN" cf "$DIR/emberhud-1.21.11.jar" -C "$BUILD/classes" .
echo "Built $DIR/emberhud-1.21.11.jar"
