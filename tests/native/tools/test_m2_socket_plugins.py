"""Portable source/staging controls. Never launch a provider or native helper."""
from __future__ import annotations
import copy
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import m2_socket_plugins as plugins
import m2_socket_runner as runner
from m2_socket_duration import source_identities, canonical_hash, make_profile, artifact_identity, validate_binding, qualify
from test_m2_socket_duration import fixture

EVIDENCE = plugins.HERE.parents[2] / "evidence/M2/replacement-activation-repair"
RESULTS = []


class SourceControls(unittest.TestCase):
    def setUp(self):
        self.assertions = 0
        self.temp = tempfile.TemporaryDirectory(prefix="ts-plugin-source-control-")
        self.root = Path(self.temp.name).resolve()
        self.config = {"condition": "SOURCE_ONLY_FIXTURE", "runtimeId": "synthetic-runtime", "sourceEpoch": "synthetic-epoch",
                       "output": str(self.root / "unrun-result.json"), "delays": [], "plan": []}
        self.composition = plugins.composition_identity()
        self.no_process = patch("subprocess.Popen", side_effect=AssertionError("process launch forbidden in source-only controls"))
        self.spawn = self.no_process.start()

    def tearDown(self):
        self.check(self.spawn.call_count == 0, "no subprocess launch attempted")
        RESULTS.append({"test": self.id(), "assertions": self.assertions})
        self.no_process.stop()
        self.temp.cleanup()

    def check(self, condition, message):
        self.assertions += 1
        self.assertTrue(condition, message)

    def rejects(self, function, message):
        try:
            function()
        except (RuntimeError, OSError, ValueError, KeyError):
            self.check(True, message)
        else:
            self.check(False, message)

    def staged(self):
        return runner.prepare_provider(self.root, self.config)

    def test_original_duplicate_and_repaired_registration_structure(self):
        original = (EVIDENCE / "original-combined-probe.ts").read_text()
        repaired = (plugins.HERE / "m2_socket_probe.ts").read_text()
        observer = (plugins.OBSERVER / "hooks/register.ts").read_text()
        self.check("import { register as registerObserver } from './observer.ts';" in original, "original imported observer registration")
        self.check("registerObserver(on, options);" in original, "original called imported registration with the same on")
        self.check(original.count("on('session.start',") == 1 and observer.count("on('session.start',") == 1, "two unmatched registrations in the original import graph")
        self.check("registerObserver" not in repaired and "observer.ts" not in repaired, "repaired probe does not import/register observer")
        self.check(repaired.count("on('session.start',") == 1 and repaired.count("\n  on(") == 1, "dedicated plugin has one intended registration")
        self.check(observer.count("on('session.start',") == 1, "original observer retains its registration")

    def test_entire_callback_and_next_forwarding_preserved(self):
        original = (EVIDENCE / "original-combined-probe.ts").read_text()
        repaired = (plugins.HERE / "m2_socket_probe.ts").read_text()
        start = "  on('session.start',"
        self.check(original[original.index(start):] == repaired[repaired.index(start):], "complete callback/task bytes unchanged")
        self.check(repaired.count("next(e)") == 1, "one original next(e) call")
        self.check("const forwarded = next(e);" in repaired and "return forwarded;" in repaired, "unchanged result forwarding")

    def test_staged_manifests_paths_modules_and_disabled_delivery(self):
        prepared = self.staged()
        observer, qualification = prepared["plugins"]["plugins"]
        self.check([p["name"] for p in (observer, qualification)] == ["threadspace-observer", "threadspace-socket-duration"], "distinct expected names")
        self.check(observer["root"] != qualification["root"], "distinct resolved roots")
        self.check(all(Path(p["root"]).resolve() == Path(p["root"]) for p in (observer, qualification)), "roots are canonical absolute paths")
        for entry in (observer, qualification):
            root = Path(entry["root"])
            manifest = json.loads((root / ".claude-plugin/plugin.json").read_text())
            self.check(manifest["name"] == entry["name"] and isinstance(manifest["version"], str), "manifest structure and identity")
            self.check(json.loads((root / "hooks/hooks.json").read_text()) == {"modules": ["./register.ts"]}, "one correct hook module")
            self.check((root / "hooks/register.ts").is_file(), "hook entry exists")
        manifest = json.loads((Path(observer["root"]) / ".claude-plugin/plugin.json").read_text())
        self.check(manifest["types"] == "./types/index.d.ts", "original declarations referenced")
        self.check(manifest["userConfig"]["captureArgv"]["default"] == [], "capture delivery remains disabled by default")
        self.check("pluginConfigs" not in json.loads((self.root / "config/settings.json").read_text()), "settings do not enable capture argv")

    def test_observer_bytes_and_only_required_qualification_files(self):
        prepared = self.staged()
        observer, qualification = prepared["plugins"]["plugins"]
        for name in plugins.OBSERVER_FILES:
            self.check((Path(observer["root"]) / name).read_bytes() == (plugins.OBSERVER / name).read_bytes(), "canonical observer byte identity: " + name)
        self.check(set(qualification["files"]) == {".claude-plugin/plugin.json", "hooks/hooks.json", "hooks/register.ts", "hooks/latency.ts", "hooks/background.ts", "hooks/config.ts"}, "qualification package contains no observer register")
        self.check((Path(qualification["root"]) / "hooks/register.ts").read_bytes() == (plugins.HERE / "m2_socket_probe.ts").read_bytes(), "dedicated probe source identity")
        plugins.verify_plugins(self.root, self.config, self.composition, prepared["plugins"])
        self.check(True, "actual staged manifest verifier accepts complete package")

    def test_exact_loader_arguments_and_no_owner_plugin_environment(self):
        before = dict(os.environ)
        with patch.dict(os.environ, {"CLAUDE_CODE_PLUGIN_DIRS": "/synthetic-unrelated-plugin", "CLAUDE_CODE_PLUGIN_SEED_DIR": "/synthetic-seed", "CLAUDE_CONFIG_DIR": "/synthetic-owner-config"}):
            prepared = self.staged()
        self.check(dict(os.environ) == before, "parent environment remains unchanged")
        argv = prepared["argv"]
        roots = [p["root"] for p in prepared["plugins"]["plugins"]]
        loaded = [argv[i + 1] for i, value in enumerate(argv) if value == "--plugin-dir"]
        self.check(loaded == roots and len(set(loaded)) == 2, "each intended root loaded exactly once")
        self.check(not any(k.startswith("CLAUDE_CODE_PLUGIN_") for k in prepared["env"]), "no duplicate/inherited plugin loading mechanism")
        self.check(prepared["env"]["CLAUDE_CONFIG_DIR"] == str(self.root / "config"), "only owned configuration")
        self.check(argv[argv.index("--settings") + 1] == str(self.root / "config/settings.json"), "owned settings path")
        self.check(argv[argv.index("--setting-sources") + 1] == "", "owner user/project/local settings excluded")
        self.check("--strict-mcp-config" in argv and argv[argv.index("--mcp-config") + 1] == '{"mcpServers":{}}', "unchanged strict empty MCP configuration")
        self.check("/synthetic-unrelated-plugin" not in argv and str(runner.PROVIDER) == argv[0], "only exact provider and intended plugin inputs")

    def test_file_replacement_omission_and_alteration_rejected_before_launch(self):
        prepared = self.staged()
        for entry in prepared["plugins"]["plugins"]:
            for name in entry["files"]:
                path = Path(entry["root"]) / name
                original = path.read_bytes()
                for mutation in ("alter", "omit", "replace"):
                    if mutation == "alter": path.write_bytes(original + b"\n")
                    elif mutation == "omit": path.unlink()
                    else: path.write_bytes(b"{}\n")
                    self.rejects(lambda: runner.start_provider(self.root, self.config, prepared), entry["role"] + ": " + name + " " + mutation)
                    path.write_bytes(original); os.chmod(path, 0o600)

    def test_manifest_config_path_alias_and_unexpected_file_controls(self):
        prepared = self.staged()
        for role in ("observer", "qualification"):
            changed = copy.deepcopy(prepared)
            entry = next(p for p in changed["plugins"]["plugins"] if p["role"] == role)
            entry["name"] = "foreign-plugin"
            self.rejects(lambda: runner.start_provider(self.root, self.config, changed), "changed manifest name: " + role)
        changed = copy.deepcopy(prepared)
        changed["plugins"]["plugins"][1]["root"] = changed["plugins"]["plugins"][0]["root"]
        self.rejects(lambda: runner.start_provider(self.root, self.config, changed), "aliased plugin root")
        changed = dict(self.config, sourceEpoch="foreign-epoch")
        self.rejects(lambda: runner.start_provider(self.root, changed, prepared), "runtime configuration changed")
        extra = Path(prepared["plugins"]["plugins"][1]["root"]) / "hooks/observer.ts"
        extra.write_text("unexpected source")
        self.rejects(lambda: runner.start_provider(self.root, self.config, prepared), "unexpected imported observer source")
        extra.unlink()
        record = self.root / "plugin-identities.json"
        record.write_text("{}")
        self.rejects(lambda: runner.start_provider(self.root, self.config, prepared), "persisted identity manifest changed")

    def test_launch_argument_environment_and_settings_substitution_rejected(self):
        prepared = self.staged()
        for mutation in ("extra-plugin", "extra-env", "owner-settings"):
            changed = copy.deepcopy(prepared)
            if mutation == "extra-plugin": changed["argv"] += ["--plugin-dir", "/synthetic-unrelated-plugin"]
            if mutation == "extra-env": changed["env"]["CLAUDE_CODE_PLUGIN_DIRS"] = "/synthetic-unrelated-plugin"
            if mutation == "owner-settings": changed["env"]["CLAUDE_CONFIG_DIR"] = "/synthetic-owner-config"
            self.rejects(lambda: runner.start_provider(self.root, self.config, changed), mutation)
        (self.root / "config/settings.json").write_text('{"pluginConfigs":{"foreign":{"options":{}}}}')
        self.rejects(lambda: runner.start_provider(self.root, self.config, prepared), "settings substitution")

    def test_disposable_settings_ownership_and_alias_controls(self):
        prepared = self.staged()
        settings = self.root / "config/settings.json"
        os.chmod(settings, 0o644)
        self.rejects(lambda: runner.start_provider(self.root, self.config, prepared), "settings permissions changed")
        os.chmod(settings, 0o600)
        cfg = self.root / "config"
        os.chmod(cfg, 0o755)
        self.rejects(lambda: runner.start_provider(self.root, self.config, prepared), "configuration directory permissions changed")
        os.chmod(cfg, 0o700)
        retained = self.root / "synthetic-foreign-config"
        cfg.rename(retained); cfg.symlink_to(retained, target_is_directory=True)
        self.rejects(lambda: runner.start_provider(self.root, self.config, prepared), "aliased disposable configuration rejected")

    def test_malformed_or_missing_canonical_staging_rejected(self):
        files = dict(plugins.qualification_files())
        for manifest in ({}, [], {"name": "fixture", "version": 1}, {"name": "fixture", "version": ""}, {"name": "fixture", "version": "1", "types": []}):
            changed = dict(files, **{".claude-plugin/plugin.json": plugins.json_bytes(manifest)})
            self.rejects(lambda: plugins.describe(changed), "malformed manifest")
        changed = dict(files); del changed["hooks/register.ts"]
        self.rejects(lambda: plugins.describe(changed), "missing module")
        changed = dict(files, **{"hooks/hooks.json": b'{"modules":["./missing.ts"]}'})
        self.rejects(lambda: plugins.describe(changed), "wrong module path")
        changed = copy.deepcopy(self.composition); changed["observer"]["files"].pop("types/index.d.ts")
        self.rejects(lambda: runner.prepare_provider(self.root, self.config, changed), "omitted canonical package identity")

    def test_both_conditions_stage_before_any_positive_process(self):
        freeze = {"identities": {**source_identities(), "runId": "synthetic-run", "bootId": "synthetic-boot", "responderId": "synthetic-responder"},
                  "runtimePlan": [{"condition": c, "runtimeId": "synthetic-" + c, "sourceEpoch": "synthetic-" + c} for c in runner.POLICY["conditions"]], "plan": []}
        prepared = runner.prepare_conditions(self.root, freeze)
        self.check(len(prepared) == 2, "both conditions constructed without launching")
        self.check([c["condition"] for _, c, _ in prepared] == runner.POLICY["conditions"], "condition order unchanged")
        attempt = self.root / "unrun-attempt"
        with patch.object(runner, "new_freeze", return_value=freeze), patch.object(runner, "prepare_provider", side_effect=RuntimeError("injected staging refusal")), patch.object(runner, "start_responder") as responder, patch("subprocess.check_output") as command:
            self.rejects(lambda: runner.run(attempt, Path("/synthetic-unrun-responder"), freeze), "staging refusal before positive entry")
            self.check(responder.call_count == 0 and command.call_count == 0, "neither responder nor provider version invoked")

    def test_original_conformance_identity_join_binds_both_plugins(self):
        report = fixture()
        profile, artifact = make_profile(report), artifact_identity(report)
        self.check(not validate_binding(report, profile, artifact)[1], "synthetic identity baseline accepted")
        for role in ("observer", "qualification"):
            for name in report["identities"]["pluginComposition"][role]["files"]:
                changed_profile, changed_artifact = copy.deepcopy(profile), copy.deepcopy(artifact)
                for value in (changed_profile, changed_artifact):
                    value["pluginComposition"][role]["files"][name] = "0" * 64
                self.check(bool(validate_binding(report, changed_profile, changed_artifact)[1]), "joint metadata substitution retains old conformance: " + role + "/" + name)
        changed = copy.deepcopy(report)
        changed["identities"]["pluginComposition"]["qualification"]["modules"] = ["./foreign.ts"]
        self.check(bool(validate_binding(changed, make_profile(report), artifact)[1]), "conformance composition substitution rejected")

    def test_generated_configuration_join_uses_original_staging(self):
        # Exercise the native-record identity branch with synthetic data only.
        report = fixture()
        report["executionKind"] = "NATIVE_INSTALLED_PROVIDER"
        prepared = runner.prepare_conditions(self.root, report["freeze"])
        report["pluginStaging"] = [{"condition": config["condition"], "manifest": inputs["plugins"],
            "manifestSha256": canonical_hash(inputs["plugins"])} for _, config, inputs in prepared]
        self.check(qualify(report)["qualified"], "synthetic complete staging baseline for native-record branch")
        for mutation in ("missing-staging", "missing-package", "reordered-conditions", "config", "module-list", "digest"):
            changed = copy.deepcopy(report)
            if mutation == "missing-staging": changed.pop("pluginStaging")
            elif mutation == "missing-package": changed["pluginStaging"][0]["manifest"]["plugins"].pop()
            elif mutation == "reordered-conditions": changed["pluginStaging"].reverse()
            elif mutation == "config":
                entry = changed["pluginStaging"][0]
                entry["manifest"]["plugins"][1]["files"]["hooks/config.ts"] = "0" * 64
                entry["manifestSha256"] = canonical_hash(entry["manifest"])
            elif mutation == "module-list": changed["pluginStaging"][0]["manifest"]["plugins"][1]["modules"] = ["./foreign.ts"]
            else: changed["pluginStaging"][0]["manifestSha256"] = "0" * 64
            verdict = qualify(changed)
            self.check(not verdict["qualified"] and any("staging" in error for error in verdict["errors"]), "original generated staging identity rejects " + mutation)


if __name__ == "__main__":
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(SourceControls)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    print(json.dumps({"kind": "PORTABLE_SOURCE_STAGING_CONTROLS", "testsRun": result.testsRun,
        "failures": len(result.failures), "errors": len(result.errors), "assertions": sum(r["assertions"] for r in RESULTS),
        "providerLaunched": False, "responderLaunched": False, "nativeActivation": "UNVERIFIED", "controls": RESULTS}, sort_keys=True))
    raise SystemExit(not result.wasSuccessful())
