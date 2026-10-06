#!/bin/bash
# Builds and signs ThreadspaceAgent.app for one channel, inside-out:
# Rust core staticlib -> Swift AppKit executable -> bundle -> codesign with the
# companion's own entitlements. The outer Tauri bundler later copies this
# already-signed bundle into Contents/Library/LoginItems and signs only the
# outer application (SPEC §18.9).
#
#   apps/agent-macos/build.sh dev|prod
#
# THREADSPACE_SIGNING_IDENTITY overrides the default: the first valid
# "Apple Development" identity in the login keychain (by SHA-1).
set -euo pipefail

CHANNEL="${1:-}"
case "$CHANNEL" in
  dev)  APP_IDENTIFIER="ai.scalinity.threadspace.dev"; AGENT_NAME="Threadspace Agent Dev" ;;
  prod) APP_IDENTIFIER="ai.scalinity.threadspace";     AGENT_NAME="Threadspace Agent" ;;
  *) echo "usage: $0 dev|prod" >&2; exit 64 ;;
esac
AGENT_IDENTIFIER="$APP_IDENTIFIER.agent"

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HERE="$ROOT/apps/agent-macos"
TARGET="aarch64-apple-darwin"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
OUT_DIR="$HERE/build/$CHANNEL"
APP="$OUT_DIR/ThreadspaceAgent.app"

IDENTITY="${THREADSPACE_SIGNING_IDENTITY:-$(security find-identity -v -p codesigning | awk '/"Apple Development/ { print $2; exit }')}"
if [ -z "$IDENTITY" ]; then
  echo "no Apple Development signing identity found" >&2
  exit 1
fi

echo "==> Rust core (release, qualification)"
cargo build --manifest-path "$ROOT/Cargo.toml" -p threadspace-agent --release --features qualification --target "$TARGET"

echo "==> Swift AppKit shell"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
swiftc -O -swift-version 5 -target arm64-apple-macos26.0 \
  -import-objc-header "$HERE/include/threadspace_agent.h" \
  "$HERE"/Sources/*.swift \
  -L "$ROOT/target/$TARGET/release" -lthreadspace_agent -liconv \
  -framework AppKit -framework UserNotifications -framework CoreServices \
  -o "$APP/Contents/MacOS/ThreadspaceAgent"

echo "==> Bundle"
sed -e "s/@AGENT_IDENTIFIER@/$AGENT_IDENTIFIER/" -e "s/@AGENT_NAME@/$AGENT_NAME/" -e "s/@VERSION@/$VERSION/" \
  "$HERE/Info.plist.in" > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist" >/dev/null
cp "$HERE/Resources/terminal-inventory.applescript" "$HERE/Resources/terminal-focus.applescript" "$APP/Contents/Resources/"
cp "$ROOT/apps/desktop/src-tauri/icons/icon.icns" "$APP/Contents/Resources/icon.icns"
printf 'APPL????' > "$APP/Contents/PkgInfo"

echo "==> Sign (hardened runtime, companion entitlements)"
codesign --force --sign "$IDENTITY" --options runtime --timestamp=none \
  --entitlements "$HERE/ThreadspaceAgent.entitlements" "$APP"
codesign --verify --strict --verbose=2 "$APP"

EXECUTABLE_SHA256="$(shasum -a 256 "$APP/Contents/MacOS/ThreadspaceAgent" | awk '{ print $1 }')"
cat > "$OUT_DIR/build-info.json" <<EOF
{
  "channel": "$CHANNEL",
  "agentIdentifier": "$AGENT_IDENTIFIER",
  "version": "$VERSION",
  "sourceCommit": "$(git -C "$ROOT" rev-parse HEAD)",
  "sourceTreeDirty": $( [ -n "$(git -C "$ROOT" status --porcelain --untracked-files=no)" ] && echo true || echo false ),
  "executableSha256": "$EXECUTABLE_SHA256",
  "qualificationBuild": true
}
EOF
echo "==> $APP"
