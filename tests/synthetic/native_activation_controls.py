#!/usr/bin/env python3
"""Run discriminating M3 mutations, restoring every source even on failure.

Run from the repository root. Uses only Python's standard library and cargo;
all output goes beneath the existing ignored target/ directory.
"""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "target/m3-verification"
OUT.mkdir(parents=True, exist_ok=True)
ENGINE = ROOT / "crates/state-engine/src/activations.rs"
ADAPTER = ROOT / "crates/provider-claude/src/activation.rs"
OBSERVER = ROOT / "crates/provider-claude/src/observer.rs"
COMMAND = ["cargo", "test", "-p", "threadspace-synthetic", "--lib", "native_activations::tests", "--", "--test-threads=1"]


def replace_once(text, old, new):
    if text.count(old) != 1:
        raise RuntimeError(f"mutation requires one source match: {old!r}")
    return text.replace(old, new, 1)


def legacy_naive(text):
    start = text.index("            } else {", text.index("let matched ="))
    end = text.index("            };", start)
    return text[:start] + "            } else { true\n" + text[end:]


def inventory_unmatched(text):
    start = text.index("for inventory in")
    return text[:start] + replace_once(text[start:], "if matches.len() == 1", "if false && matches.len() == 1")


CONTROLS = [
    ("legacy-binding-replay", ROOT / "crates/state-engine/src/reduce.rs", lambda s: replace_once(s, "if !self.state.activation_facts.is_empty()\n            && let", "if let")),
    ("native-supersession", ENGINE, lambda s: replace_once(s, 'execution\n                .end_reasons\n                .insert("NATIVE_SESSION_SWITCHED".into());', 'let _ = &execution;')),
    ("exact-generation-end", ENGINE, lambda s: replace_once(s, "if matched", "if false && matched")),
    ("ambiguous-legacy-end", ENGINE, legacy_naive),
    ("inventory-convergence", ENGINE, inventory_unmatched),
    ("originating-turn", ENGINE, lambda s: replace_once(s, "turn.execution_ids = ids;", "turn.execution_ids = BTreeSet::new(); let _ = ids;")),
    ("compaction", ADAPTER, lambda s: replace_once(s, 'Some("startup" | "resume" | "clear")', 'Some("startup" | "resume" | "clear" | "compact")')),
    ("reload-is-not-opening", OBSERVER, lambda s: replace_once(s, "if starts && record.engine_identity && generation.is_some()", 'if (starts && record.engine_identity || event == "session.start" && record.phase == "bootstrap") && generation.is_some()')),
]
results = []
for name, path, mutate in CONTROLS:
    original = path.read_text()
    try:
        changed = mutate(original)
        if changed == original:
            raise RuntimeError("mutation did not change source")
        path.write_text(changed)
        command = ["cargo", "test", "-p", "threadspace-synthetic", "--test", "pre_m3_journal"] if name == "legacy-binding-replay" else COMMAND
        result = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        (OUT / f"control-{name}.log").write_text(result.stdout)
        discriminated = result.returncode != 0 and "test result: FAILED" in result.stdout
        results.append({"rule": name, "command": " ".join(command), "exitStatus": result.returncode, "detectedByFailingAssertion": discriminated})
    finally:
        path.write_text(original)
summary = {"passed": all(r["detectedByFailingAssertion"] for r in results), "controls": results}
(OUT / "controls.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(summary, indent=2))
raise SystemExit(0 if summary["passed"] else 1)
