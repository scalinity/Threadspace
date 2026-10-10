#!/usr/bin/env python3
"""Bounded F4 launch controls. No inference, installed integration, or live store writes.

Use the exact retained helper and provider. Keep first executions of new inodes;
never describe missing entry/exit stamps as measured. Raw provider output stays
in the explicitly supplied private output directory.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import uuid


class Timespec(ctypes.Structure):
    _fields_ = [("seconds", ctypes.c_long), ("nanoseconds", ctypes.c_long)]


def native_ns():
    value = Timespec()
    if ctypes.CDLL(None).clock_gettime(8, ctypes.byref(value)):
        raise OSError("CLOCK_UPTIME_RAW failed")
    return value.seconds * 1_000_000_000 + value.nanoseconds


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def save(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + "\n")


def batch(epoch, record=False):
    records = [{
        "schemaVersion": 1, "observationId": str(uuid.uuid4()),
        "adapterId": "threadspace-observer", "adapterVersion": "0.1.0",
        "sourceEpoch": epoch, "sequenceMeaning": "OBSERVER_CAPTURE",
        "callbackEntrySequence": "1", "callbackResultSequence": "2",
        "phase": "result", "nativeEvent": "turn.start",
        "dispatchOrigin": {"plugin": "engine", "tier": "core"}, "engineDispatch": True,
        "sessionId": "session-1", "sessionIdSource": "classic.SessionStart",
        "sessionGeneration": 1, "nativeTurnId": "turn-1",
        "payload": {"echoedTurnId": "turn-1"},
    }] if record else []
    return json.dumps({"receiptVersion": 1, "kind": "mod-batch", "sourceEpoch": epoch,
                       "droppedRecords": 0, "records": records})


def identity(path):
    stat = Path(path).stat()
    return {"sha256": digest(path), "device": stat.st_dev, "inode": stat.st_ino}


def direct(helper, store, stdin):
    argv = [str(helper), "mod-batch", "--store-dir", str(store), "--budget-ms", "80"]
    before = native_ns()
    child = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    spawned = native_ns()
    stdout, stderr = child.communicate(stdin.encode(), timeout=4)
    reaped = native_ns()
    return {
        "parentLaunchRequestNs": str(before), "successfulSpawnReturnNs": str(spawned),
        "parentReceiptAndReapNs": str(reaped), "helperEntryNs": None,
        "helperReadyToAnswerNs": None, "finalProcessExitNs": None,
        "exitIsBracketedByParentReap": True,
        "launchToReapMs": (reaped - before) / 1e6,
        "spawnCallMs": (spawned - before) / 1e6,
        "spawnReturnToReapMs": (reaped - spawned) / 1e6,
        "pid": child.pid, "exitCode": child.returncode,
        "receipt": json.loads(stdout), "stderr": stderr.decode(),
        "phaseLimitation": "Installed phase gate excludes --store-dir; no native entry stamp exists for this disposable store.",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--helper", type=Path, required=True)
    parser.add_argument("--provider", type=Path, required=True)
    parser.add_argument("--private-output", type=Path, required=True)
    args = parser.parse_args()
    root = args.private_output.resolve()
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    helper, provider = args.helper.resolve(strict=True), args.provider.resolve(strict=True)
    epoch = str(uuid.uuid4())
    summary = {"kind": "F4_BOUNDED_STARTUP_CONTROL", "clock": "CLOCK_UPTIME_RAW",
               "bootId": subprocess.check_output(["sysctl", "-n", "kern.bootsessionuuid"], text=True).strip(),
               "sourceCommit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
               "helper": identity(helper), "provider": identity(provider),
               "inferenceRequestsIssued": 0, "direct": [], "host": None,
               "durableCommitControl": "NOT_RUN: no clock-qualified repair; retained actual COMMIT controls remain source-matched.",
               "notAnF4AcceptanceCohort": True}
    for copy in range(2):
        executable = root / f"direct-helper-{copy}"
        shutil.copy2(helper, executable)
        before_identity = identity(executable)
        for call in range(2):
            store = root / f"direct-store-{copy}-{call}"
            store.mkdir(mode=0o700)
            row = direct(executable, store, batch(epoch, record=True))
            row.update({"copy": copy, "call": call, "firstExecutionOfAcquiredInode": call == 0,
                        "fileIdentity": before_identity,
                        "readySpoolRecords": len(list((store / "capture-spool/ready").glob("*.json")))})
            assert identity(executable) == before_identity
            summary["direct"].append(row)
        # Empty-batch warm control separates record processing/spool from launch.
        store = root / f"empty-store-{copy}"
        store.mkdir(mode=0o700)
        row = direct(executable, store, batch(epoch))
        row.update({"copy": copy, "call": 2, "emptyBatchWarmControl": True,
                    "fileIdentity": before_identity, "firstExecutionOfAcquiredInode": False})
        summary["direct"].append(row)

    plugin = root / "host-plugin"
    (plugin / ".claude-plugin").mkdir(parents=True)
    (plugin / "hooks").mkdir()
    save(plugin / ".claude-plugin/plugin.json", {"name": "threadspace-f4-startup-control", "version": "0.1.0"})
    save(plugin / "hooks/hooks.json", {"modules": ["./register.ts"]})
    calls = []
    for copy in range(2):
        executable = root / f"host-helper-{copy}"
        shutil.copy2(helper, executable)
        for call in range(2):
            store = root / f"host-store-{copy}-{call}"
            store.mkdir(mode=0o700)
            calls.append({"copy": copy, "call": call,
                          "fileIdentity": identity(executable),
                          "firstExecutionOfAcquiredInode": call == 0,
                          "argv": [str(executable), "mod-batch", "--store-dir", str(store), "--budget-ms", "230"],
                          "stdin": batch(epoch, record=True)})
    save(root / "calls.json", calls)
    # This writer supplies causal native before/after stamps, not JS subtraction.
    writer = root / "writer.py"
    writer.write_text("import sys,json,ctypes\nfrom pathlib import Path\n"
                      "class T(ctypes.Structure): _fields_=[('s',ctypes.c_long),('n',ctypes.c_long)]\n"
                      "v=T(); assert ctypes.CDLL(None).clock_gettime(8,ctypes.byref(v))==0\n"
                      "at=str(v.s*1000000000+v.n)\n"
                      "if len(sys.argv)==1: print(json.dumps({'nativeNs':at}))\n"
                      "else:\n data=json.load(sys.stdin);data['nativeWriterEntryNs']=at;Path(sys.argv[1]).write_text(json.dumps(data));print('{}')\n")
    host_output = root / "host-result.json"
    script = """let started = false;
export function register(on) {
  on('session.start', ($, e, next) => {
    const result = next(e);
    if (!started) { started = true; void probe($); }
    return result;
  });
}
async function probe($) {
  const rows = [];
  const globals = {bun: typeof Bun === 'undefined' ? null : {version:Bun.version, revision:Bun.revision},
    process: typeof process === 'undefined' ? null : {version:process.version, bun:process.versions?.bun, uv:process.versions?.uv},
    performanceNowSource: String(performance.now), timeOrigin:performance.timeOrigin};
  for (const call of CALLS) {
    const lower = await $.process.run(WRITER, {timeoutMs:1000});
    const beginMs = performance.now();
    let reply, error;
    try { reply = await $.process.run(call.argv, {stdin:call.stdin, timeoutMs:250}); }
    catch (e) { error = {name:e?.name, message:String(e?.message).slice(0,180)}; }
    const endMs = performance.now();
    rows.push({...call, argv:undefined, stdin:undefined, nativeBeforeRequestNs:JSON.parse(lower.stdout).nativeNs,
      hostBeginMs:beginMs, hostEndMs:endMs, hostSpanMs:endMs-beginMs, reply, error});
    await $.process.run([...WRITER, OUTPUT], {stdin:JSON.stringify({globals, rows}), timeoutMs:1000});
  }
}
"""
    script = script.replace("CALLS", json.dumps(calls)).replace("WRITER", json.dumps([sys.executable, str(writer)])).replace("OUTPUT", json.dumps(str(host_output)))
    (plugin / "hooks/register.ts").write_text(script)
    config = root / "provider-config"
    config.mkdir(mode=0o700)
    save(config / "settings.json", {"hooks": {}, "disableAllHooks": False})
    environment = os.environ.copy()
    environment["CLAUDE_CONFIG_DIR"] = str(config)
    environment["CLAUDE_CODE_PLUGIN_DIRS"] = str(plugin)
    argv = [str(provider), "--print", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose",
            "--no-session-persistence", "--plugin-dir", str(plugin), "--settings", str(config / "settings.json"),
            "--setting-sources", "", "--strict-mcp-config", "--mcp-config", '{"mcpServers":{}}']
    out = (root / "provider-stdout.log").open("wb")
    err = (root / "provider-stderr.log").open("wb")
    before = native_ns()
    child = subprocess.Popen(argv, cwd=root, env=environment, stdin=subprocess.PIPE, stdout=out, stderr=err, start_new_session=True)
    spawned = native_ns()
    request = {"type": "control_request", "request_id": str(uuid.uuid4()), "request": {"subtype": "initialize"}}
    child.stdin.write((json.dumps(request) + "\n").encode())
    child.stdin.flush()
    deadline = time.monotonic() + 20
    complete = False
    while time.monotonic() < deadline and child.poll() is None:
        if host_output.exists():
            try:
                complete = len(json.loads(host_output.read_text())["rows"]) == len(calls)
            except (ValueError, KeyError):
                pass
            if complete:
                break
        time.sleep(.05)
    # Only this Popen handle is acted on; no Terminal or unrelated PID is touched.
    if child.poll() is None:
        child.stdin.close()
        try:
            child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            child.terminate()
            child.wait(timeout=3)
    out.close()
    err.close()
    summary["providerController"] = {"launchRequestNs":str(before), "successfulSpawnReturnNs":str(spawned),
                                    "parentReapNs":str(native_ns()), "pid":child.pid,
                                    "exitCode":child.returncode, "initializeControlOnly":True,
                                    "complete":complete}
    if host_output.exists():
        summary["host"] = json.loads(host_output.read_text())
        for row in summary["host"]["rows"]:
            row["helperEntryNs"] = None
            row["helperReadyToAnswerNs"] = None
            row["finalProcessExitNs"] = None
    save(root / "summary.json", summary)
    print(json.dumps({"directMs": [round(row["launchToReapMs"],3) for row in summary["direct"]],
                      "hostComplete":complete, "hostRows": len(summary["host"]["rows"]) if summary["host"] else 0}))


if __name__ == "__main__":
    main()
