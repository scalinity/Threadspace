include!("d0008_guard.rs");

fn main() {
    // D-0008 / C-04: no build of this crate, packaged or not, proceeds on a
    // Tauri, tao or Wry other than the set the containment was qualified on.
    let lockfile = d0008_lockfile(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    println!("cargo:rerun-if-changed={lockfile}");
    if let Err(refusal) = d0008_guard(&lockfile) {
        panic!("{refusal}");
    }

    // A nonempty application command manifest makes the app-command ACL apply
    // to local custom commands; without it they bypass the capability check
    // (SPEC §18.6).
    let manifest = tauri_build::AppManifest::new().commands(&[
        "ui_connect",
        "ui_ack",
        "ui_disconnect",
        "ui_query",
        "ui_action",
    ]);
    let attributes = tauri_build::Attributes::new().app_manifest(manifest);
    tauri_build::try_build(attributes).expect("Threadspace Tauri build failed");
}
