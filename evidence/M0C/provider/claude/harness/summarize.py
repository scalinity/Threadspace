"""Prints a compact view of a native run's batches.jsonl."""
import json
import sys

path = sys.argv[1]
rows = [json.loads(line) for line in open(path) if line.strip()]
if not rows:
    print("no batches")
    sys.exit(0)
t0 = rows[0]["at"]
epochs = []
for row in rows:
    env = row["envelope"] or {}
    epoch = env.get("sourceEpoch")
    if epoch not in epochs:
        epochs.append(epoch)
    print(f"batch +{row['at'] - t0:7.3f}s epoch#{epochs.index(epoch)} {row['outcome']:6} records={len(env.get('records', []))} "
          f"bytes={row['stdinBytes']} dropped={env.get('droppedRecords')}")
    for rec in env.get("records", []):
        payload = rec.get("payload", {})
        core = payload.get("core")
        extra = {k: v for k, v in payload.items() if k != "core"}
        print(f"   {rec.get('callbackEntrySequence'):>3}/{str(rec.get('callbackResultSequence') or ''):>3} {rec['phase']:9} "
              f"{rec['nativeEvent']:21} origin={rec['dispatchOrigin']['plugin']}/{rec['dispatchOrigin']['tier']} "
              f"session={rec.get('sessionId')}({rec.get('sessionIdSource')},g{rec.get('sessionGeneration')}) "
              f"turn={rec.get('nativeTurnId')} actor={rec.get('actorNativeId')} "
              f"{json.dumps(extra, separators=(',', ':'))} core={json.dumps(core, separators=(',', ':'))}")
