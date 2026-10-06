#!/bin/bash
# Builds a signed Threadspace.app for one channel (SPEC §18.9, inside-out):
#   1. ThreadspaceAgent.app — Rust core + Swift shell, signed with its own entitlements
#   2. Threadspace.app — Tauri 3 bundle that copies the signed companion into
#      Contents/Library/LoginItems before signing only the outer application
#   3. independent verification of both signatures
#
#   tests/native/build-app.sh dev|prod
#
# dev  -> "Threadspace Dev.app" (ai.scalinity.threadspace.dev), the stable
#         development host bundle that registers the development companion.
# prod -> "Threadspace.app" (ai.scalinity.threadspace).
# Both are qualification builds (`--features qualification`).
set -euo pipefail

CHANNEL="${1:-}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
case "$CHANNEL" in
  dev)  PRODUCT="Threadspace Dev"; CONFIG=(--config src-tauri/tauri.dev.conf.json) ;;
  prod) PRODUCT="Threadspace";     CONFIG=() ;;
  *) echo "usage: $0 dev|prod" >&2; exit 64 ;;
esac

IDENTITY="${THREADSPACE_SIGNING_IDENTITY:-$(security find-identity -v -p codesigning | awk '/"Apple Development/ { print $2; exit }')}"
export THREADSPACE_SIGNING_IDENTITY="$IDENTITY"

"$ROOT/apps/agent-macos/build.sh" "$CHANNEL"

echo "==> Threadspace ($CHANNEL) Tauri bundle"
cd "$ROOT/apps/desktop"
APPLE_SIGNING_IDENTITY="$IDENTITY" npx --no-install tauri build "${CONFIG[@]}" \
  --target aarch64-apple-darwin --bundles app --features qualification

APP="$ROOT/target/aarch64-apple-darwin/release/bundle/macos/$PRODUCT.app"
AGENT="$APP/Contents/Library/LoginItems/ThreadspaceAgent.app"
echo "==> Verify nested companion, then outer application"
codesign --verify --strict --verbose=2 "$AGENT"
codesign --verify --strict --verbose=2 "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"
echo "==> $APP"
