#!/usr/bin/env python3
"""One fixed installed-host duration qualification, initialization only.

All files and the provider process are disposable and explicitly owned. No
inference, integration install, capture-helper call or persistent service.
Native before/after markers enclose both JS endpoints; timers are workload,
never elapsed-time oracles. The declared model is engineering uncertainty,
not a universal rate certificate.
"""
import argparse
import ctypes
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import uuid
from m2_latency import integer, number

POLICY = {
    "schemaVersion": 3, "method": "composed-duration-v1",
    "requestedDelaysMs": [0, 100, 250], "runs": 1, "repeatsUntilPass": False,
    "maximumBracketWidthMs": 60, "maximumObserverAllowanceMs": 30,
    "maximumObservedClockStepMs": 1, "maximumDurationMs": 400,
    "guardMs": 1, "nativeAdditiveMs": .001, "outwardStepMs": .001,
    "observerModel": "max(0, native upper elapsed enclosure minus observer elapsed) over fixed checks, plus largest observed positive read step and 1 ms guard, rounded outward",
    "scope": "pinned installed executable and guarded host performance.now wrapper; same-epoch intervals within 0..400 ms, same boot/build, awake, no reset or lost census",
    "limitation": "fixed engineering conformance and uncertainty; empirical consistency is not a universal rate bound",
}


def digest(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def canonical_hash(value): return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
def save(path, value): Path(path).write_text(json.dumps(value, indent=2) + "\n")
def boot(): return subprocess.check_output(["/usr/sbin/sysctl", "-n", "kern.bootsessionuuid"], text=True).strip()


def qualify(report, policy=POLICY):
    errors, details = [], []
    if report.get("complete") is not True or report.get("bootBefore") != report.get("bootAfter") or report.get("powerDiscontinuity") is not False:
        errors.append("incomplete runtime, changed boot or suspend discontinuity")
    rows = report.get("rows", [])
    if [row.get("requestedDelayMs") for row in rows] != policy["requestedDelaysMs"]:
        errors.append("missing or reordered fixed conformance checks")
    maximum_under, maximum_step = 0, 0
    last = report.get("first")
    try:
        last = number(last)
        if "guarded" not in report.get("wrapper", "") or "helpers.now" not in report.get("wrapper", ""): raise ValueError("changed installed guarded host clock wrapper")
    except (TypeError, ValueError) as error:
        last = -1
        errors.append(str(error))
    for row in rows:
        try:
            readings = row["readings"]
            if len(readings) != 258 or any(not isinstance(v, (int, float)) or isinstance(v, bool) or not 0 <= v < float("inf") for v in readings) or any(a > b for a, b in zip([last] + readings, readings)):
                raise ValueError("nonfinite, negative or reset clock")
            last = readings[-1]
            positive = [b - a for a, b in zip(readings[:256], readings[1:256]) if b > a]
            if not positive: raise ValueError("unresolved coarsening")
            step = max(positive)
            maximum_step = max(maximum_step, step)
            c, r = readings[-2:]
            clo, chi, rlo, rhi = [integer(row[key]["nativeNs"]) for key in ("captureLower", "captureUpper", "endLower", "endUpper")]
            if not clo <= chi <= rlo <= rhi or any(row[key].get("bootId") != report["bootBefore"] for key in ("captureLower", "captureUpper", "endLower", "endUpper")):
                raise ValueError("invalid native causal bracket/boot")
            low, high, measured = (rlo - chi) / 1e6, (rhi - clo) / 1e6, r - c
            width = high - low
            if measured > high + step or measured < low - step: raise ValueError("inconsistent elapsed millisecond units")
            if width > policy["maximumBracketWidthMs"] or step > policy["maximumObservedClockStepMs"] or measured > policy["maximumDurationMs"]:
                raise ValueError("unusable precision or duration scope")
            maximum_under = max(maximum_under, high - measured)
            details.append({"requestedDelayMs": row["requestedDelayMs"], "observerElapsedMs": measured,
                "nativeLowerMs": low, "nativeUpperMs": high, "bracketWidthMs": width, "observedPositiveStepMs": step})
        except (KeyError, TypeError, ValueError) as error: errors.append(str(error))
    allowance = math.ceil((maximum_under + maximum_step + policy["guardMs"]) / policy["outwardStepMs"]) * policy["outwardStepMs"]
    if allowance > policy["maximumObserverAllowanceMs"]: errors.append("duration uncertainty exceeds predeclared useful allowance")
    return {"qualified": not errors, "observerAdditiveMs": allowance if not errors else None,
        "nativeAdditiveMs": policy["nativeAdditiveMs"], "outwardStepMs": policy["outwardStepMs"],
        "maximumDurationMs": policy["maximumDurationMs"], "universalRateBound": False,
        "scope": "INSTALLED_ENGINEERING_DURATION", "nativeClock": "CLOCK_UPTIME_RAW", "observerClock": "performance.now",
        "checks": details, "errors": errors}


PROBE = """
import ctypes,json,subprocess,sys
from pathlib import Path
class T(ctypes.Structure): _fields_=[('s',ctypes.c_long),('n',ctypes.c_long)]
boot=subprocess.check_output(['/usr/sbin/sysctl','-n','kern.bootsessionuuid'],text=True).strip()
v=T();assert ctypes.CDLL(None).clock_gettime(8,ctypes.byref(v))==0
stamp={'nativeNs':str(v.s*1000000000+v.n),'bootId':boot,'clock':'CLOCK_UPTIME_RAW'}
if len(sys.argv)>1:
    data=json.load(sys.stdin);data['nativeFinalStamp']=stamp;Path(sys.argv[1]).write_text(json.dumps(data));print('{}')
else: print(json.dumps(stamp))
"""

MOD = """
import { createLatencyMeasurement } from './latency.ts';
let started=false;
const runtimeId=crypto.randomUUID();
const meter=createLatencyMeasurement(['/unused-capture-helper','mod-batch','--qualification-latency'],runtimeId);
export function register(on) {
  on('session.start',($,e,next)=>{
    const first=meter.readCapture();
    const result=next(e);
    if(!started){started=true;void check($,first)}
    return result;
  });
}
async function check($,first) {
  const rows=[];
  const native=async()=>{
    const result=await $.process.run(PROBE_ARGV,{timeoutMs:1000});
    if(result.exitCode!==0||result.isStdoutTruncated)throw new Error('missing native marker');
    return JSON.parse(result.stdout);
  };
  const publish=async(complete,error)=>$.process.run([...PROBE_ARGV,OUTPUT],{stdin:JSON.stringify({runtimeId,first,rows,complete,error,
    wrapper:String(performance.now),sourceClock:'performance.now'}),timeoutMs:1000});
  try {
    for(const delay of DELAYS){
      const readings=Array.from({length:256},()=>meter.readCapture());
      const captureLower=await native();
      const c=meter.readCapture();
      const captureUpper=await native();
      if(delay>0)await new Promise(resolve=>$.clock.after(delay,resolve));
      const endLower=await native();
      const r=meter.readCapture();
      const endUpper=await native();
      readings.push(c,r);
      rows.push({requestedDelayMs:delay,readings,captureLower,captureUpper,endLower,endUpper});
      await publish(false,null);
    }
    await publish(true,null);
  }catch(e){await publish(false,String(e?.message).slice(0,160))}
}
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--provider", type=Path, required=True)
    parser.add_argument("--private-output", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[3]
    root = args.private_output.resolve()
    root.mkdir(parents=True, mode=0o700, exist_ok=False)
    provider = args.provider.resolve(strict=True)
    before_boot = boot()
    identities = {"providerSha256": digest(provider), "adapterSha256": canonical_hash({name: digest(repo / "packages/provider-mod/hooks" / name) for name in ("register.ts", "latency.ts", "delivery.ts", "ownership.ts")}),
        "policySha256": canonical_hash(POLICY), "probeSourceSha256": hashlib.sha256(PROBE.encode()).hexdigest(),
        "osBuild": subprocess.check_output(["/usr/bin/sw_vers", "-buildVersion"], text=True).strip(),
        "power": subprocess.check_output(["/usr/bin/pmset", "-g", "custom"], text=True).strip()}
    save(root / "policy.json", POLICY)  # Frozen before the only provider launch.
    save(root / "identities.json", identities)
    plugin = root / "plugin"
    (plugin / ".claude-plugin").mkdir(parents=True)
    (plugin / "hooks").mkdir()
    save(plugin / ".claude-plugin/plugin.json", {"name": "threadspace-duration-conformance", "version": "0.1.0"})
    save(plugin / "hooks/hooks.json", {"modules": ["./register.ts"]})
    shutil.copyfile(repo / "packages/provider-mod/hooks/latency.ts", plugin / "hooks/latency.ts")
    marker = root / "marker.py"
    marker.write_text(PROBE)
    output = root / "host-result.json"
    mod = MOD.replace("PROBE_ARGV", json.dumps([sys.executable, str(marker)])).replace("OUTPUT", json.dumps(str(output))).replace("DELAYS", json.dumps(POLICY["requestedDelaysMs"]))
    (plugin / "hooks/register.ts").write_text(mod)
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
    began = time.monotonic()
    wall_began = time.time()
    child = subprocess.Popen(argv, cwd=root, env=environment, stdin=subprocess.PIPE, stdout=out, stderr=err, start_new_session=True)
    child.stdin.write((json.dumps({"type": "control_request", "request_id": str(uuid.uuid4()), "request": {"subtype": "initialize"}}) + "\n").encode())
    child.stdin.flush()
    complete = False
    try:
        while time.monotonic() - began < 20 and child.poll() is None:
            if output.exists():
                try:
                    report = json.loads(output.read_text())
                    complete = report.get("complete") is True or report.get("error") is not None
                except ValueError: pass
                if complete: break
            time.sleep(.05)
    finally:
        if child.poll() is None:
            child.stdin.close()
            try: child.wait(timeout=3)
            except subprocess.TimeoutExpired: child.terminate(); child.wait(timeout=3)
        out.close(); err.close()
    report = json.loads(output.read_text()) if output.exists() else {"rows": [], "complete": False}
    report.update(bootBefore=before_boot, bootAfter=boot(), policy=POLICY, identities=identities,
        initializeControlOnly=True, providerExitCode=child.returncode,
        powerDiscontinuity=abs((time.time() - wall_began) - (time.monotonic() - began)) > .01)
    save(root / "conformance.json", report)
    result = qualify(report)
    profile = {**identities, **result, "bootId": before_boot, "conformanceSha256": canonical_hash(report),
        "conformanceRuntimeId": report.get("runtimeId"), "wrapper": report.get("wrapper")}
    published = {"schemaVersion": 3, "kind": "FIXED_INSTALLED_DURATION_CONFORMANCE", "profile": profile,
        "report": report, "executionKind": "NATIVE_INSTALLED_PROVIDER", "acceptanceCohortRun": False}
    save(args.report, published)
    print(json.dumps({"qualified": result["qualified"], "errors": result["errors"], "checks": result["checks"]}))
    return 0 if result["qualified"] else 1


if __name__ == "__main__": raise SystemExit(main())
