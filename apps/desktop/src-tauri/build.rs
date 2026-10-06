// A nonempty application command manifest makes the app-command ACL apply to
// local custom commands; without it they bypass the capability check
// (SPEC §18.6).
fn main() {
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
