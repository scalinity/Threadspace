#!/bin/bash
# D-0008 guard controls (C-04). Positive: an ordinary build of the desktop
# crate on the qualified dependency set succeeds. Negative: in a disposable
# export of HEAD whose lockfile moves tao to 0.37.1, the same ordinary build
# fails in build.rs with the D-0008 message before any desktop artifact
# exists. The repository's own lockfile is never touched.
# Usage: tests/native/d0008-guard.sh <evidence dir>
set -u
out=${1:?usage: d0008-guard.sh <evidence dir>}
root=$(git rev-parse --show-toplevel)
mkdir -p "$out"
work=$(mktemp -d "${TMPDIR:-/tmp}/d0008-negative.XXXXXX")
trap 'rm -rf "$work"' EXIT

head=$(git -C "$root" rev-parse HEAD)
(cd "$root" && cargo build -p threadspace-desktop) >"$out/positive.log" 2>&1
positive=$?

mkdir -p "$work/src"
git -C "$root" archive HEAD | tar -x -C "$work/src"
(cd "$work/src" && cargo update -p tao --precise 0.37.1 --offline) >"$out/negative-update.log" 2>&1
grep -A1 '^name = "tao"' "$work/src/Cargo.lock" >"$out/negative-lock-tao.txt"
touch "$work/start.stamp"
(cd "$work/src" && CARGO_TARGET_DIR="$work/target" cargo build -p threadspace-desktop) >"$work/negative.log" 2>&1
negative=$?
grep -E 'failed to run custom build command|panicked at|D-0008 / C-04' "$work/negative.log" \
  | sed "s|$work|<disposable>|g" >"$out/negative.log"
artifacts=$(find "$work/target" -newer "$work/start.stamp" -type f \
  \( -name 'threadspace-desktop' -o -name 'threadspace_desktop*' -o -name 'libthreadspace_desktop*' \) | wc -l | tr -d ' ')
message=$(grep -c 'Remove the D-0008 containment first' "$out/negative.log")

pass=false
[ "$positive" -eq 0 ] && [ "$negative" -ne 0 ] && [ "$artifacts" -eq 0 ] && [ "$message" -ge 1 ] && pass=true
cat >"$out/summary.json" <<EOF
{
  "area": "d0008-guard",
  "sourceCommit": "$head",
  "positiveBuildExit": $positive,
  "negativeLockTao": "0.37.1",
  "negativeBuildExit": $negative,
  "negativeD0008Message": $([ "$message" -ge 1 ] && echo true || echo false),
  "desktopArtifactsProduced": $artifacts,
  "pass": $pass
}
EOF
cat "$out/summary.json"
