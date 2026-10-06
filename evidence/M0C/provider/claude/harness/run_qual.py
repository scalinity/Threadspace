"""Runs the M0C engine-test-kit qualification and saves the raw output.

  run_qual.py <repo> <out-dir>

The only edit to the output is the repository's absolute path, written as
<repo>; each file's header states the command, its exit code and when.
"""
import datetime
import os
import subprocess
import sys

repo, out_dir = sys.argv[1], sys.argv[2]
claude = os.path.expanduser("~/.local/bin/claude")
mod = os.path.join(repo, "packages", "provider-mod")
version = subprocess.run([claude, "--version"], capture_output=True, text=True).stdout.strip()

runs = [
    ("validate-output.txt", [claude, "plugin", "validate", mod], None),
    ("validate-output.json", [claude, "plugin", "validate", "--json", mod], None),
    ("test-output.txt", [claude, "plugin", "test", mod], None),
]
for name, argv, cwd in runs:
    started = datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")
    out = subprocess.run(argv, capture_output=True, text=True, cwd=cwd, timeout=900)
    body = (out.stdout + out.stderr).replace(repo, "<repo>")
    shown = " ".join(a.replace(repo, "<repo>").replace(os.path.expanduser("~"), "~") for a in argv)
    if name.endswith(".json"):
        text = body
    else:
        text = f"# command: {shown}\n# claude --version: {version}\n# started: {started}\n# exit code: {out.returncode}\n\n{body}"
    open(os.path.join(out_dir, name), "w").write(text)
    print(name, "exit", out.returncode)
