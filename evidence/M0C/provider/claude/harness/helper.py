"""Stand-in for `threadspace-hook mod-batch` in the M0C native session runs.

argv: helper.py mod-batch <run-dir>
Appends one line per invocation to <run-dir>/batches.jsonl. With the policy
file <run-dir>/policy reading `fail-first-epoch`, every batch from the first
source epoch it sees exits 1 with no receipt; any other epoch is committed.
"""
import json
import os
import sys
import time


def main():
    run_dir = sys.argv[2]
    raw = sys.stdin.read()
    now = time.time()
    try:
        envelope = json.loads(raw)
    except ValueError:
        envelope = None
    epoch = envelope.get("sourceEpoch") if isinstance(envelope, dict) else None

    policy_path = os.path.join(run_dir, "policy")
    policy = open(policy_path).read().strip() if os.path.exists(policy_path) else "commit"
    fail = False
    if policy == "fail-first-epoch" and epoch:
        first_path = os.path.join(run_dir, "first-epoch")
        if not os.path.exists(first_path):
            with open(first_path, "w") as handle:
                handle.write(epoch)
        fail = open(first_path).read().strip() == epoch

    with open(os.path.join(run_dir, "batches.jsonl"), "a") as handle:
        handle.write(json.dumps({
            "at": now,
            "pid": os.getpid(),
            "stdinBytes": len(raw.encode("utf-8")),
            "outcome": "fail" if fail else "commit",
            "envelope": envelope,
        }) + "\n")

    if fail or not isinstance(envelope, dict):
        sys.exit(1)
    results = [{"observationId": r["observationId"], "status": "COMMITTED"} for r in envelope["records"]]
    sys.stdout.write(json.dumps({"receiptVersion": 1, "results": results}))


main()
