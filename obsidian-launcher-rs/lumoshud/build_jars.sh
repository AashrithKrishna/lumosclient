#!/usr/bin/env bash
# Builds lumoshud-<mc>.jar for every version in gradle.properties (mc_versions) with Gradle,
# unless each jar is already newer than the mod's sources. Prints the jars' paths.
#   lumoshud/build_jars.sh [--force]
set -euo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
VERSIONS="$(sed -n 's/^mc_versions=//p' "$DIR/gradle.properties" | tr ',' ' ')"

stale=0
[[ "${1:-}" == "--force" ]] && stale=1
for v in $VERSIONS; do
  jar="$DIR/lumoshud-$v.jar"
  if [[ ! -f "$jar" ]] || [[ -n "$(find "$DIR/src" "$DIR/build.gradle" "$DIR/settings.gradle" "$DIR/gradle.properties" -newer "$jar" -type f 2>/dev/null | head -1)" ]]; then
    stale=1
  fi
done

if [[ $stale == 1 ]]; then
  # Loom needs a JDK 21 or newer to run Gradle.
  if [[ -z "${JAVA_HOME:-}" ]] && [[ -x /usr/libexec/java_home ]]; then
    JAVA_HOME="$(/usr/libexec/java_home -v 21+ 2>/dev/null || true)"
    [[ -n "$JAVA_HOME" ]] && export JAVA_HOME
  fi
  (cd "$DIR" && ./gradlew build --console=plain -q >&2)
  # Gradle leaves a jar alone when its contents didn't change; mark it current for the check above.
  for v in $VERSIONS; do [[ -f "$DIR/lumoshud-$v.jar" ]] && touch "$DIR/lumoshud-$v.jar"; done
fi

for v in $VERSIONS; do
  [[ -f "$DIR/lumoshud-$v.jar" ]] || { echo "error: $DIR/lumoshud-$v.jar wasn't built" >&2; exit 1; }
  echo "$DIR/lumoshud-$v.jar"
done
