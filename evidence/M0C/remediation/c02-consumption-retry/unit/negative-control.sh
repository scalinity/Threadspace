#!/bin/bash
# C-02B negative controls for the writer's consumption tests, each in a
# disposable git worktree (the working checkout is never modified):
#   guard-removed  the candidate's Writer::consume without the arm that
#                  retries a repeated consumption whose removal has not
#                  committed (every repeat then answers Done at once);
#   pre-fix        Writer::consume as it was at the base, with the
#                  candidate's test module appended (its fixture's
#                  `unrecorded` field mapped to the base's `backlog_stale`).
# Both must fail the counterexample tests: the repeat answers Done while the
# committed backlog still lists the intent.
#
#   negative-control.sh <candidate-commit> <base-commit> <scratch-dir>
set -euo pipefail
CANDIDATE="${1:?candidate commit}"
BASE="${2:?base commit}"
SCRATCH="${3:?scratch directory}"
ROOT="$(git rev-parse --show-toplevel)"
WRITER=apps/agent-macos/core/src/writer.rs
GUARD='            None if self.unrecorded.iter().any(|id| id == intent_id) => true,'
export CARGO_TARGET_DIR="$SCRATCH/c02b-negative-control-target"

run() {
  local name="$1" commit="$2" tree="$SCRATCH/c02b-negative-control-$1"
  git -C "$ROOT" worktree add --detach "$tree" "$commit" >/dev/null
  case "$name" in
    guard-removed)
      grep -qxF "$GUARD" "$tree/$WRITER"
      grep -vxF "$GUARD" "$tree/$WRITER" > "$tree/$WRITER.mutated"
      mv "$tree/$WRITER.mutated" "$tree/$WRITER" ;;
    pre-fix)
      git -C "$ROOT" show "$CANDIDATE:$WRITER" \
        | sed -n '/^\/\/\/ C-02B: `Done` for `IntentConsumed`/,$p' \
        | sed 's/            unrecorded: Vec::new(),/            backlog_stale: false,/' >> "$tree/$WRITER" ;;
  esac
  echo "## $name: $WRITER at $(git -C "$tree" rev-parse --short HEAD), changed as follows"
  git -C "$tree" diff --unified=0 -- "$WRITER" | grep -E '^[-+][^-+]' | head -3
  echo "## cargo test -p threadspace-agent writer::tests"
  (cd "$tree" && cargo test -p threadspace-agent writer::tests 2>&1) \
    | grep -E '^test |^test result|^second:|^\[Err|^  left:|^ right:' || true
  git -C "$ROOT" worktree remove --force "$tree"
  echo
}

run guard-removed "$CANDIDATE"
run pre-fix "$BASE"
