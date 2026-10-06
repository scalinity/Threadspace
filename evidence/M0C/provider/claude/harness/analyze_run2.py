"""Derives the reload/timer-cancellation figures from a run 2 directory."""
import json
import sys

run_dir, out_path = sys.argv[1], sys.argv[2]
rows = [json.loads(line) for line in open(f"{run_dir}/batches.jsonl") if line.strip()]
run = json.load(open(f"{run_dir}/run2.json"))
start = run["timeline"]["start"]
rel = lambda t: round(t - start, 3)

reload_line = next((l for l in run["streamLines"] if l["subtype"] == "ui_log" and "reloaded" in l["line"]), None)
order = []
for row in rows:
    epoch = row["envelope"]["sourceEpoch"]
    if epoch not in order:
        order.append(epoch)
attempts = {epoch: [] for epoch in order}
for row in rows:
    env = row["envelope"]
    attempts[env["sourceEpoch"]].append({"at": rel(row["at"]), "outcome": row["outcome"], "records": len(env["records"])})

first = attempts[order[0]]
gaps = [round(b["at"] - a["at"], 3) for a, b in zip(first, first[1:])]
reload_at = rel(reload_line["at"]) if reload_line else None
next_due = round(first[-1]["at"] + 4.0, 3)
exit_at = rel(run["timeline"]["exit"])
expected_if_live = []
due, delay = next_due, 4.0
while due < exit_at:
    expected_if_live.append(round(due, 3))
    delay = min(delay * 2, 5.0)
    due += delay
second = attempts[order[1]] if len(order) > 1 else []
first_epoch_records = rows[0]["envelope"]["records"] if rows else []

analysis = {
    "clock": "seconds after the orchestrator started the claude process",
    "editAt": rel(run["timeline"]["edit"]),
    "reloadUiLogAt": reload_at,
    "reloadUiLog": reload_line["line"] if reload_line else None,
    "exitAt": exit_at,
    "epochs": [{"epochIndex": i, "sourceEpoch": e, "attempts": attempts[e]} for i, e in enumerate(order)],
    "firstEpochRetryGapsSeconds": gaps,
    "firstEpochPendingRetryDueAt": next_due,
    "firstEpochAttemptsAfterReload": [a for a in first if reload_at is not None and a["at"] > reload_at],
    "firstEpochAttemptsExpectedIfTimerSurvived": expected_if_live,
    "secondEpochFirstRecords": [
        {"phase": r["phase"], "nativeEvent": r["nativeEvent"], "sessionId": r.get("sessionId"), "sessionIdSource": r.get("sessionIdSource")}
        for r in (rows[[row["envelope"]["sourceEpoch"] for row in rows].index(order[1])]["envelope"]["records"] if len(order) > 1 else [])
    ],
    "secondEpochEvents": sorted({r["nativeEvent"] for row in rows if row["envelope"]["sourceEpoch"] == (order[1] if len(order) > 1 else None) for r in row["envelope"]["records"]}),
    "firstEpochRecordsNeverAcknowledged": len({r["observationId"] for row in rows if row["envelope"]["sourceEpoch"] == order[0] for r in row["envelope"]["records"]}),
}
json.dump(analysis, open(out_path, "w"), indent=1)
print(json.dumps({k: v for k, v in analysis.items() if k not in ("epochs", "reloadUiLog")}, indent=1))
