#!/usr/bin/env python3
"""F2 CLI smoke against the installed Dev bundle and owned disposable settings.

No provider Session, UI, Terminal, service, production bundle or owner settings launch.
Every failure retains the acquired configuration and integration for diagnosis.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import time
import uuid


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def inventory(roots):
    return {
        str(file): {"sha256": digest(file), "mode": file.stat().st_mode}
        for root in roots
        for file in root.rglob("*")
        if file.is_file() and not file.is_symlink()
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--claude", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if sys.platform != "darwin":
        raise SystemExit("This runner requires macOS.")
    app = args.app.resolve(strict=True)
    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    if info.get("CFBundleIdentifier") != "ai.scalinity.threadspace.dev":
        raise SystemExit("Only the Dev bundle is permitted.")
    binary = app / "Contents/MacOS/Threadspace"
    owned = Path.home() / "Library/Application Support/ai.scalinity.threadspace.dev/integrations/claude"
    if (owned / "record.json").exists():
        raise SystemExit("An existing integration occupies the Dev slot; no mutation performed.")
    output = args.output.resolve()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    scratch = Path(tempfile.mkdtemp(prefix="ts-m2-f2-native-"))
    config = scratch / "config"
    config.mkdir(mode=0o700)
    settings = config / "settings.json"
    original = b'{"foreign":{"kept":true},"hooks":{"SessionStart":[{"matcher":"foreign","hooks":[{"type":"command","command":"/usr/bin/true"}]}]},"env":{"CLAUDE_CODE_PLUGIN_DIRS":"/opt/foreign/plugins","FOREIGN":"kept"}}\n'
    calls, cases = [], []
    original_bundle_hash = digest(binary)
    result = {
        "nativeExecution": True, "bundleIdentifier": info["CFBundleIdentifier"],
        "outerSha256": original_bundle_hash, "runnerSha256": digest(Path(__file__)),
        "scratch": str(scratch), "calls": calls, "cases": cases, "pass": False,
        "scope": "Native CLI integration only; observer execution and atomic-write fault controls are separate evidence.",
    }

    def save():
        (output / "summary.json").write_text(json.dumps(result, indent=2) + "\n")

    def cli(operation, scope="user", directory=config):
        assert digest(binary) == original_bundle_hash, "Installed executable changed"
        argv = [str(binary), "--integration", operation, "--config-dir", str(directory), "--scope", scope]
        start = time.monotonic()
        run = subprocess.run(argv, capture_output=True, text=True, timeout=20)
        value = json.loads(run.stdout.strip().splitlines()[-1])
        calls.append({"argv": argv, "exitCode": run.returncode, "elapsedMs": round((time.monotonic() - start) * 1000, 3), "result": value, "stderr": run.stderr})
        save()
        return value

    def install():
        settings.write_bytes(original)
        value = cli("install")
        assert value["ok"], value
        record = value["detail"]
        assert record["identity"]["configDir"] == str(config)
        assert record["identity"]["ownedDir"] == str(owned)
        assert record["identity"]["agentIdentifier"] == "ai.scalinity.threadspace.dev.agent"
        assert record["scope"] == "user"
        helper = owned / "bin/threadspace-hook"
        bundled_helper = app / "Contents/Library/LoginItems/ThreadspaceAgent.app/Contents/MacOS/threadspace-hook"
        assert digest(helper) == digest(bundled_helper)
        mod = Path(record["pluginDir"])
        bundled_mod = app / "Contents/Library/LoginItems/ThreadspaceAgent.app/Contents/Resources/provider-mod"
        for name in ["register.ts", "ownership.ts", "delivery.ts", "latency.ts"]:
            assert digest(mod / "hooks" / name) == digest(bundled_mod / "hooks" / name)
        applied = json.loads(settings.read_bytes())
        assert applied["foreign"] == {"kept": True}
        assert applied["hooks"]["SessionStart"][0] == json.loads(original)["hooks"]["SessionStart"][0]
        assert applied["env"]["CLAUDE_CODE_PLUGIN_DIRS"].startswith("/opt/foreign/plugins:")
        return record, applied

    try:
        record, applied = install()
        before = inventory([owned, config])
        second = cli("install")
        assert second["ok"]
        # Installation time is refreshed by the CLI; ownership, resources,
        # settings and the original rollback point must remain identical.
        assert {key: value for key, value in record.items() if key != "installedAtMs"} == {
            key: value for key, value in second["detail"].items() if key != "installedAtMs"
        }
        after = inventory([owned, config])
        record_path = str(owned / "record.json")
        assert {key: value for key, value in before.items() if key != record_path} == {
            key: value for key, value in after.items() if key != record_path
        }, "Reinstall changed resources or settings"
        removed = cli("uninstall")
        assert removed["ok"] and removed["detail"]["complete"]
        assert settings.read_bytes() == original
        cases.append({"case": "unchanged-install-reinstall-remove", "pass": True, "byteIdenticalRestore": True})
        save()

        for condition in ["matcher", "timeout", "command", "type", "plugin-directory"]:
            record, applied = install()
            changed = json.loads(json.dumps(applied))
            group = changed["hooks"]["SessionStart"][-1]
            hook = group["hooks"][0]
            if condition == "matcher":
                group["matcher"] = "owner-modified"
            elif condition == "timeout":
                hook["timeout"] = 23
            elif condition == "command":
                hook["command"] += " --owner-modified"
            elif condition == "type":
                hook["type"] = "owner-modified"
            else:
                changed["env"]["CLAUDE_CODE_PLUGIN_DIRS"] += "/owner-modified"
            settings.write_text(json.dumps(changed) + "\n")
            helper = owned / "bin/threadspace-hook"
            mod = Path(record["pluginDir"])
            resources = inventory([helper.parent, mod])
            partial = cli("uninstall")
            assert partial["ok"] and partial["detail"]["complete"] is False
            assert (owned / "record.json").exists()
            assert inventory([helper.parent, mod]) == resources
            current = json.loads(settings.read_bytes())
            assert current["foreign"] == applied["foreign"]
            assert current["env"]["FOREIGN"] == "kept"
            before = inventory([owned, config])
            assert cli("install")["ok"] is False
            assert inventory([owned, config]) == before
            if condition == "plugin-directory":
                assert current["env"]["CLAUDE_CODE_PLUGIN_DIRS"] == changed["env"]["CLAUDE_CODE_PLUGIN_DIRS"]
                current["env"]["CLAUDE_CODE_PLUGIN_DIRS"] = applied["env"]["CLAUDE_CODE_PLUGIN_DIRS"]
            else:
                assert current["hooks"]["SessionStart"] == changed["hooks"]["SessionStart"]
                current["hooks"]["SessionStart"] = applied["hooks"]["SessionStart"]
            settings.write_text(json.dumps(current) + "\n")
            final = cli("uninstall")
            assert final["ok"] and final["detail"]["complete"]
            assert not helper.exists() and not mod.exists() and not (owned / "record.json").exists()
            final_settings = json.loads(settings.read_bytes())
            assert final_settings["foreign"] == {"kept": True}
            assert final_settings["hooks"]["SessionStart"] == json.loads(original)["hooks"]["SessionStart"]
            assert final_settings["env"] == json.loads(original)["env"]
            cases.append({"case": condition, "pass": True, "partialComplete": False, "resourcesRetained": True, "conflictResolved": True})
            save()

        record, applied = install()
        other = scratch / "other"
        other.mkdir(mode=0o700)
        (other / "settings.json").write_bytes(b'{"foreignOther":true}\n')
        for scope, directory in [("session", config), ("user", other)]:
            for operation in ["status", "install", "uninstall"]:
                before = inventory([owned, scratch])
                assert cli(operation, scope, directory)["ok"] is False
                assert inventory([owned, scratch]) == before
                cases.append({"case": f"refuse-{operation}-{scope}-{'same' if directory == config else 'different'}-config", "pass": True, "byteAndModeEquality": True})

        # Execute the staged assets without a model request or live-store write.
        provider_config = scratch / "provider-config"
        provider_config.mkdir(mode=0o700)
        environment = os.environ.copy()
        environment["CLAUDE_CONFIG_DIR"] = str(provider_config)
        provider = args.claude.resolve(strict=True)
        version = subprocess.run([str(provider), "--version"], env=environment, capture_output=True, text=True, timeout=20)
        assert version.returncode == 0 and version.stdout.strip() == "2.1.295 (Claude Code)"
        mod = Path(record["pluginDir"])
        helper = owned / "bin/threadspace-hook"
        before_assets = {"helper": digest(helper), "register": digest(mod / "hooks/register.ts")}
        validate = subprocess.run([str(provider), "plugin", "validate", str(mod)], env=environment, capture_output=True, text=True, timeout=30)
        assert validate.returncode == 0, validate.stderr + validate.stdout
        store = scratch / "isolated-hook-store"
        store.mkdir(mode=0o700)
        fixture = {"hook_event_name": "SessionStart", "session_id": str(uuid.uuid4()), "cwd": str(scratch)}
        hook = subprocess.run([str(helper), "hook", "--agent", "ai.scalinity.threadspace.dev.agent", "--store-dir", str(store)], input=json.dumps(fixture), capture_output=True, text=True, timeout=5)
        assert hook.returncode == 0 and hook.stdout == "" and hook.stderr == ""
        assert before_assets == {"helper": digest(helper), "register": digest(mod / "hooks/register.ts")}
        cases.append({"case": "staged-native-assets", "pass": True, "providerVersion": version.stdout.strip(), "providerSha256": digest(provider), "pluginValidateStdout": validate.stdout, "pluginValidateStderr": validate.stderr, "helperExitCode": hook.returncode, "helperSilent": True, "assetHashesStable": True, "isolatedHookStore": True, "fixtureHasNoClaudeParent": True, "sessionAuthorityClaimed": False})
        assert cli("uninstall")["detail"]["complete"]
        assert settings.read_bytes() == original
        result["pass"] = True
        result["integrationRemoved"] = not (owned / "record.json").exists()
        result["fixtureRetained"] = True
        save()
    except Exception as error:
        result["error"] = f"{type(error).__name__}: {error}"
        result["fixtureRetained"] = True
        save()
        raise
    print(json.dumps({"pass": result["pass"], "cases": len(cases), "calls": len(calls), "output": str(output)}))


if __name__ == "__main__":
    main()
