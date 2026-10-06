import os, re, shutil, subprocess, sys

src = sys.argv[1]
work = sys.argv[2]
claude = os.path.expanduser("~/.local/bin/claude")

MUTATIONS = [
    ("receipt size cap removed", "hooks/delivery.ts",
     "if (utf8Length(result.stdout) > LIMITS.receiptBytes) return undefined", ""),
    ("exit code ignored", "hooks/delivery.ts",
     "if (result.exitCode !== 0 || result.isStdoutTruncated) return undefined", "if (result.isStdoutTruncated) return undefined"),
    ("no backoff doubling", "hooks/delivery.ts",
     "retryMs = Math.min(retryMs * 2, LIMITS.retryMaxMs)", "retryMs = LIMITS.retryMinMs"),
    ("next called twice on tool.call", "hooks/register.ts",
     "return pass(ctx, () => next(e), () => next.trace, result => ({\n      resultKind",
     "return pass(ctx, async () => { await next(e); return next(e) }, () => next.trace, result => ({\n      resultKind"),
    ("session read at result time", "hooks/register.ts",
     "sessionId: ctx.sessionId,", "sessionId: session.id,"),
    ("worktree tools not excluded", "hooks/register.ts",
     "on('tool.call', { tool: /^(?!(?:EnterWorktree|ExitWorktree)$)/ },", "on('tool.call',"),
    ("queue byte bound raised to 80 MiB", "hooks/delivery.ts",
     "queueBytes: 8 * 1024 * 1024,", "queueBytes: 80 * 1024 * 1024,"),
    ("queue record bound raised to 20480", "hooks/delivery.ts",
     "queueRecords: 2048,", "queueRecords: 20480,"),
    ("batch record bound raised to 1280", "hooks/delivery.ts",
     "batchRecords: 128,", "batchRecords: 1280,"),
    ("batch byte bound raised to 640 KiB", "hooks/delivery.ts",
     "batchBytes: 64 * 1024,", "batchBytes: 640 * 1024,"),
    ("prompt text leaked", "hooks/register.ts",
     "      wait: e.wait,\n", "      wait: e.wait,\n      text: e.text,\n"),
]

for name, rel, old, new in MUTATIONS:
    if os.path.exists(work):
        shutil.rmtree(work)
    shutil.copytree(src, work)
    path = os.path.join(work, rel)
    text = open(path, encoding="utf-8").read()
    if old not in text:
        print(f"{name}: MUTATION NOT APPLIED")
        continue
    open(path, "w", encoding="utf-8").write(text.replace(old, new, 1))
    out = subprocess.run([claude, "plugin", "test", work], capture_output=True, text=True, timeout=600)
    combined = out.stdout + out.stderr
    fails = [l.strip() for l in combined.splitlines() if l.startswith("(fail)")]
    summary = [l.strip() for l in combined.splitlines() if re.match(r"^\s*\d+ (pass|fail)$", l)]
    print(f"{name}: exit={out.returncode} {' '.join(summary)}")
    for f in fails:
        print(f"    {f}")
shutil.rmtree(work, ignore_errors=True)
