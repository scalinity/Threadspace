"""Source-bound staging of the two disposable qualification plugins; no processes."""
from __future__ import annotations
import hashlib
import json
import os
from pathlib import Path

HERE = Path(__file__).resolve().parent
OBSERVER = HERE.parents[2] / "packages/provider-mod"
OBSERVER_FILES = (
    ".claude-plugin/plugin.json", "hooks/hooks.json", "hooks/register.ts",
    "hooks/delivery.ts", "hooks/latency.ts", "hooks/ownership.ts", "types/index.d.ts",
)
QUALIFICATION_MANIFEST = {"name": "threadspace-socket-duration", "version": "2.0.0"}
QUALIFICATION_HOOKS = {"modules": ["./register.ts"]}
ROOT_NAMES = ("observer-plugin", "duration-plugin")


def json_bytes(value):
    return (json.dumps(value, sort_keys=True, indent=2, allow_nan=False) + "\n").encode()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def qualification_files():
    return {
        ".claude-plugin/plugin.json": json_bytes(QUALIFICATION_MANIFEST),
        "hooks/hooks.json": json_bytes(QUALIFICATION_HOOKS),
        "hooks/register.ts": (HERE / "m2_socket_probe.ts").read_bytes(),
        "hooks/latency.ts": (OBSERVER / "hooks/latency.ts").read_bytes(),
        "hooks/background.ts": (HERE / "m2_socket_background.ts").read_bytes(),
    }


def package_files(config=None):
    observer = {name: (OBSERVER / name).read_bytes() for name in OBSERVER_FILES}
    qualification = qualification_files()
    if config is not None:
        qualification["hooks/config.ts"] = ("export const config = " + json.dumps(config, allow_nan=False) + ";\n").encode()
    return observer, qualification


def describe(files):
    manifest = json.loads(files[".claude-plugin/plugin.json"])
    hooks = json.loads(files["hooks/hooks.json"])
    if not isinstance(manifest, dict) or not isinstance(manifest.get("name"), str) or not manifest["name"] or not isinstance(manifest.get("version"), str) or not manifest["version"]:
        raise RuntimeError("malformed plugin manifest")
    if hooks != {"modules": ["./register.ts"]}:
        raise RuntimeError("unexpected hook module list")
    if "hooks/register.ts" not in files:
        raise RuntimeError("missing plugin entry point")
    if "types" in manifest and (not isinstance(manifest["types"], str) or manifest["types"].removeprefix("./") not in files):
        raise RuntimeError("missing observer declarations")
    return {"name": manifest["name"], "version": manifest["version"], "modules": hooks["modules"],
            "files": {name: digest(data) for name, data in files.items()}}


def composition_identity():
    observer, qualification = package_files()
    manifest = json.loads(observer[".claude-plugin/plugin.json"])
    if manifest["name"] != "threadspace-observer" or manifest.get("userConfig", {}).get("captureArgv", {}).get("default") != []:
        raise RuntimeError("changed observer identity or disabled-delivery default")
    result = {"schemaVersion": 1, "observer": describe(observer), "qualification": describe(qualification)}
    if result["observer"]["name"] == result["qualification"]["name"]:
        raise RuntimeError("plugin names must differ")
    return result


def expected_manifest(root, config, composition):
    if composition != composition_identity():
        raise RuntimeError("changed canonical plugin sources/configuration")
    root = Path(root).resolve()
    packages = []
    for role, dirname, files in zip(("observer", "qualification"), ROOT_NAMES, package_files(config)):
        packages.append({"role": role, "root": str(root / dirname), **describe(files)})
    return {"schemaVersion": 1, "composition": composition, "plugins": packages}


def validate_manifest(manifest, root, config, composition):
    if manifest != expected_manifest(root, config, composition):
        raise RuntimeError("changed plugin identity manifest/source/configuration")
    paths = [Path(p["root"]).resolve() for p in manifest["plugins"]]
    if len(set(paths)) != 2:
        raise RuntimeError("plugin roots must differ")


def write_private(path, data):
    with Path(path).open("xb") as f:
        os.chmod(path, 0o600)
        f.write(data); f.flush(); os.fsync(f.fileno())


def stage_plugins(root, config, composition):
    root = Path(root).resolve(strict=True)
    if root.stat().st_uid != os.getuid() or root.stat().st_mode & 0o777 != 0o700:
        raise RuntimeError("plugin staging directory is not private and owned")
    manifest = expected_manifest(root, config, composition)
    for plugin, files in zip(manifest["plugins"], package_files(config)):
        target = Path(plugin["root"])
        target.mkdir(mode=0o700)
        for directory in sorted({str(Path(name).parent) for name in files}):
            (target / directory).mkdir(mode=0o700)
        for name, data in files.items():
            write_private(target / name, data)
    write_private(root / "plugin-identities.json", json_bytes(manifest))
    verify_plugins(root, config, composition, manifest)
    return manifest


def verify_plugins(root, config, composition, manifest):
    root = Path(root).resolve(strict=True)
    validate_manifest(manifest, root, config, composition)
    if (root / "plugin-identities.json").read_bytes() != json_bytes(manifest):
        raise RuntimeError("changed persisted plugin identity manifest")
    for plugin in manifest["plugins"]:
        target = Path(plugin["root"])
        if target.is_symlink() or target.stat().st_uid != os.getuid() or target.stat().st_mode & 0o777 != 0o700:
            raise RuntimeError("unowned or aliased plugin root")
        actual = {str(p.relative_to(target)) for p in target.rglob("*") if p.is_file()}
        if actual != set(plugin["files"]):
            raise RuntimeError("missing or unexpected plugin files")
        for p in target.rglob("*"):
            if p.is_symlink() or p.stat().st_uid != os.getuid() or p.stat().st_mode & 0o777 != (0o700 if p.is_dir() else 0o600):
                raise RuntimeError("unprotected or aliased plugin resource")
        for name, sha in plugin["files"].items():
            if digest((target / name).read_bytes()) != sha:
                raise RuntimeError("changed staged plugin source/configuration: " + name)
