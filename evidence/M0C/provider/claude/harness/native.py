"""M0C native-session harness for the Claude observer mod.

  native.py setup   <mod-src> <run-dir>   copy the mod (hooks + manifest) and
                                          point its captureArgv default at helper.py
  native.py run1    <run-dir>             one `claude -p` turn: load + lifecycle
  native.py run2    <run-dir>             one stream-json `claude -p` session, two
                                          turns, a hot reload between them
  native.py collect <run-dir> <out-dir> <label>
  native.py cleanup <run-dir>

Every run uses --model haiku, no tools, no session persistence, project-only
setting sources and no MCP servers. Nothing writes any Claude settings file:
the capture argv reaches the mod as its manifest default in the copy.
"""
import hashlib
import json
import os
import queue
import shutil
import subprocess
import sys
import threading
import time

CLAUDE = os.path.expanduser("~/.local/bin/claude")
HELPER = os.path.join(os.path.dirname(os.path.abspath(__file__)), "helper.py")
HOME = os.path.expanduser("~")
BASE = ["-p", "--model", "haiku", "--tools", "", "--no-session-persistence",
        "--setting-sources", "project", "--strict-mcp-config"]


def sha256(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


def setup(src, run_dir):
    assert run_dir.startswith("/private/tmp/claude-501/"), run_dir
    os.makedirs(run_dir)
    mod = os.path.join(run_dir, "mod")
    shutil.copytree(src, mod, ignore=shutil.ignore_patterns("tests"))
    manifest_path = os.path.join(mod, ".claude-plugin", "plugin.json")
    manifest = json.load(open(manifest_path))
    manifest["userConfig"]["captureArgv"]["default"] = ["/usr/bin/python3", "-I", HELPER, "mod-batch", run_dir]
    json.dump(manifest, open(manifest_path, "w"), indent=2)
    os.makedirs(os.path.join(run_dir, "work"))
    print(json.dumps({
        "run_dir": run_dir,
        "register.ts": sha256(os.path.join(mod, "hooks", "register.ts")),
        "delivery.ts": sha256(os.path.join(mod, "hooks", "delivery.ts")),
        "src register.ts": sha256(os.path.join(src, "hooks", "register.ts")),
        "src delivery.ts": sha256(os.path.join(src, "hooks", "delivery.ts")),
    }, indent=1))


def run1(run_dir):
    mod = os.path.join(run_dir, "mod")
    argv = [CLAUDE] + BASE + ["--plugin-dir", mod, "--output-format", "json",
                              "--debug-file", os.path.join(run_dir, "debug-run1.log"),
                              "Reply with exactly the word: ok"]
    started = time.time()
    out = subprocess.run(argv, cwd=os.path.join(run_dir, "work"), stdin=subprocess.DEVNULL,
                         capture_output=True, text=True, timeout=240)
    try:
        parsed = json.loads(out.stdout)
        result = {k: parsed.get(k) for k in ("type", "subtype", "is_error", "num_turns", "duration_ms", "result")}
    except ValueError:
        result = {"unparsed": out.stdout[:500]}
    summary = {"argv": [a.replace(HOME, "~") for a in argv], "startedAt": started, "endedAt": time.time(),
               "returncode": out.returncode, "result": result, "stderr": out.stderr[:2000]}
    json.dump(summary, open(os.path.join(run_dir, "run1.json"), "w"), indent=1)
    print(json.dumps(summary, indent=1))


def run3(run_dir):
    mod = os.path.join(run_dir, "mod")
    base = [a for a in BASE]
    base[base.index("--tools") + 1] = "Agent"
    prompt = ("Call the Agent tool exactly once with subagent_type general-purpose, description 'say ok' "
              "and prompt 'Reply with exactly the word: ok. Use no tools.' Then reply with exactly the word: done")
    argv = [CLAUDE] + base + ["--plugin-dir", mod, "--output-format", "json",
                              "--debug-file", os.path.join(run_dir, "debug-run3.log"), prompt]
    started = time.time()
    out = subprocess.run(argv, cwd=os.path.join(run_dir, "work"), stdin=subprocess.DEVNULL,
                         capture_output=True, text=True, timeout=300)
    try:
        parsed = json.loads(out.stdout)
        result = {k: parsed.get(k) for k in ("type", "subtype", "is_error", "num_turns", "duration_ms", "result")}
    except ValueError:
        result = {"unparsed": out.stdout[:500]}
    summary = {"argv": [a.replace(HOME, "~") for a in argv], "startedAt": started, "endedAt": time.time(),
               "returncode": out.returncode, "result": result, "stderr": out.stderr[:2000]}
    json.dump(summary, open(os.path.join(run_dir, "run3.json"), "w"), indent=1)
    print(json.dumps(summary, indent=1))


def run2(run_dir):
    mod = os.path.join(run_dir, "mod")
    open(os.path.join(run_dir, "policy"), "w").write("fail-first-epoch")
    argv = [CLAUDE] + BASE + ["--plugin-dir", mod, "--input-format", "stream-json",
                              "--output-format", "stream-json", "--verbose",
                              "--debug-file", os.path.join(run_dir, "debug-run2.log")]
    env = dict(os.environ, CLAUDE_CODE_PLUGIN_DIR_WATCH="1")
    proc = subprocess.Popen(argv, cwd=os.path.join(run_dir, "work"), env=env, stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
    lines = queue.Queue()
    kept = []

    def reader():
        for line in proc.stdout:
            lines.put(line)
        lines.put(None)

    threading.Thread(target=reader, daemon=True).start()

    def send(text):
        proc.stdin.write(json.dumps({"type": "user", "message": {"role": "user", "content": text}}) + "\n")
        proc.stdin.flush()

    def wait_result(limit):
        deadline = time.time() + limit
        while time.time() < deadline:
            try:
                line = lines.get(timeout=1)
            except queue.Empty:
                continue
            if line is None:
                return None
            try:
                item = json.loads(line)
            except ValueError:
                continue
            kind = item.get("type")
            if kind in ("system", "result") or "reload" in line or "ui_log" in line:
                kept.append({"at": time.time(), "type": kind, "subtype": item.get("subtype"),
                             "line": line.strip()[:600].replace(HOME, "~")})
            if kind == "result":
                return item
        return None

    def epochs():
        path = os.path.join(run_dir, "batches.jsonl")
        if not os.path.exists(path):
            return []
        return [json.loads(l)["envelope"]["sourceEpoch"] for l in open(path) if l.strip()]

    timeline = {"start": time.time()}
    send("Reply with exactly the word: one")
    first = wait_result(180)
    timeline["result1"] = time.time()
    time.sleep(3)
    timeline["edit"] = time.time()
    with open(os.path.join(mod, "hooks", "register.ts"), "a") as handle:
        handle.write("\n// M0C native reload marker\n")
    first_epoch = open(os.path.join(run_dir, "first-epoch")).read().strip() if os.path.exists(os.path.join(run_dir, "first-epoch")) else None
    deadline = time.time() + 20
    while time.time() < deadline and not any(e != first_epoch for e in epochs()):
        wait_result(0.5)
    timeline["epoch2Seen"] = time.time() if any(e != first_epoch for e in epochs()) else None
    observe_until = time.time() + 16
    while time.time() < observe_until:
        wait_result(0.5)
    timeline["send2"] = time.time()
    send("Reply with exactly the word: two")
    second = wait_result(180)
    timeline["result2"] = time.time()
    time.sleep(3)
    proc.stdin.close()
    try:
        proc.wait(timeout=60)
    except subprocess.TimeoutExpired:
        proc.kill()
    timeline["exit"] = time.time()
    stderr = proc.stderr.read()
    summary = {
        "argv": [a.replace(HOME, "~") for a in argv],
        "env": {"CLAUDE_CODE_PLUGIN_DIR_WATCH": "1"},
        "timeline": timeline,
        "returncode": proc.returncode,
        "result1": {k: (first or {}).get(k) for k in ("subtype", "is_error", "num_turns", "result")},
        "result2": {k: (second or {}).get(k) for k in ("subtype", "is_error", "num_turns", "result")},
        "streamLines": kept,
        "stderr": stderr[:2000].replace(HOME, "~"),
    }
    json.dump(summary, open(os.path.join(run_dir, "run2.json"), "w"), indent=1)
    print(json.dumps({k: summary[k] for k in ("timeline", "returncode", "result1", "result2")}, indent=1))


def collect(run_dir, out_dir, label):
    os.makedirs(out_dir, exist_ok=True)
    for name in (f"{label}.json",):
        src = os.path.join(run_dir, name)
        if os.path.exists(src):
            shutil.copy(src, os.path.join(out_dir, name))
    batches = os.path.join(run_dir, "batches.jsonl")
    if os.path.exists(batches):
        text = open(batches).read().replace(run_dir, "<run-dir>").replace(HOME, "~")
        open(os.path.join(out_dir, f"{label}-batches.jsonl"), "w").write(text)
    debug = os.path.join(run_dir, f"debug-{label}.log")
    if os.path.exists(debug):
        keep = [l.replace(run_dir, "<run-dir>").replace(HOME, "~") for l in open(debug, errors="replace")
                if "threadspace-observer" in l or "reload" in l.lower() or "plugin-dir" in l.lower()]
        open(os.path.join(out_dir, f"{label}-debug-excerpt.log"), "w").write("".join(keep[:400]))
    types = os.path.join(run_dir, "mod", ".claude-plugin", "types", "claude-code", "index.d.ts")
    if os.path.exists(types):
        info = {"path": "<mod copy>/.claude-plugin/types/claude-code/index.d.ts",
                "firstLine": open(types).readline().strip(), "sha256": sha256(types),
                "lines": sum(1 for _ in open(types))}
        json.dump(info, open(os.path.join(out_dir, f"{label}-generated-types.json"), "w"), indent=1)
        print(json.dumps(info, indent=1))


def cleanup(run_dir):
    assert run_dir.startswith("/private/tmp/claude-501/ts-m0c-claude-native-"), run_dir
    shutil.rmtree(run_dir)
    print("removed", run_dir)


if __name__ == "__main__":
    command, args = sys.argv[1], sys.argv[2:]
    {"setup": setup, "run1": run1, "run2": run2, "run3": run3, "collect": collect, "cleanup": cleanup}[command](*args)
