// D-0008 / C-04 dependency guard. `build.rs` runs it on every build of this
// crate; `window::tests` runs it again as defence in depth. Both `include!`
// this file, so there is one copy of the qualified set and of the check.

/// The dependency set `window::release_window_on_close` was qualified on
/// (D-0008), each from crates.io.
const D0008_QUALIFIED: [(&str, &str); 4] = [
    ("tao", "0.37.0"),
    ("wry", "0.57.0"),
    ("tauri", "3.0.0-alpha.4"),
    ("tauri-runtime-wry", "3.0.0-alpha.4"),
];

const D0008_REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";

/// The workspace lockfile, from this crate's manifest directory.
fn d0008_lockfile(manifest_dir: &str) -> String {
    format!("{manifest_dir}/../../../Cargo.lock")
}

/// What `lock` (a `Cargo.lock`) resolves differently from the qualified set:
/// each guarded crate must resolve exactly once, at its qualified version,
/// from crates.io (a path or patched copy at the same version could carry an
/// ownership fix).
fn d0008_moved(lock: &str) -> Option<String> {
    let moved: Vec<String> = D0008_QUALIFIED
        .iter()
        .filter_map(|&(name, qualified)| {
            let resolved: Vec<String> = lock
                .split("[[package]]")
                .filter(|package| package.contains(&format!("\nname = \"{name}\"\n")))
                .map(|package| {
                    let field = |key: &str| {
                        package
                            .lines()
                            .find_map(|line| line.strip_prefix(key))
                            .map_or("none", |value| value.trim_matches('"'))
                    };
                    format!("{} from {}", field("version = "), field("source = "))
                })
                .collect();
            (resolved != [format!("{qualified} from {D0008_REGISTRY}")])
                .then(|| format!("{name} resolves to {resolved:?}, qualified {qualified}"))
        })
        .collect();
    (!moved.is_empty()).then(|| moved.join("; "))
}

/// Refuses, with the D-0008 instruction, a lockfile that cannot be read or
/// that resolves anything but the qualified set.
fn d0008_guard(lockfile: &str) -> Result<(), String> {
    let moved = match std::fs::read_to_string(lockfile) {
        Ok(lock) => d0008_moved(&lock),
        Err(error) => Some(format!("cannot read {lockfile}: {error}")),
    };
    match moved {
        None => Ok(()),
        Some(moved) => Err(format!(
            "D-0008 / C-04 containment guard: {moved}. \
             `release_window_on_close` (src/window.rs) balances an NSWindow reference \
             that only tao 0.37.0 leaves unowned; on any other Tauri, tao or Wry it can \
             become a double release (a crash). Remove the D-0008 containment first, then \
             requalify the view-recovery gate (C-04) on the updated dependency without it; \
             only then change D0008_QUALIFIED (d0008_guard.rs)."
        )),
    }
}
