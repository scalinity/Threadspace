//! Desktop-side build and platform identity for the Diagnostics query.

use std::path::Path;

use threadspace_contracts::diagnostics::DesktopDiagnostics;
use threadspace_relay::paths::redact_home;
use threadspace_surfaces_macos::{process, terminal::plist_string};

const WEBKIT_INFO: &str =
    "/System/Library/Frameworks/WebKit.framework/Versions/A/Resources/Info.plist";

pub fn desktop(app_identifier: &str, app_version: &str) -> DesktopDiagnostics {
    let (major, minor, patch) = process::os_product_version().unwrap_or((0, 0, 0));
    DesktopDiagnostics {
        app_identifier: app_identifier.to_owned(),
        app_version: app_version.to_owned(),
        tauri_version: tauri::VERSION.to_owned(),
        os_product_version: format!("{major}.{minor}.{patch}"),
        os_build_version: process::os_build_version().unwrap_or_default(),
        webkit_version: plist_string(Path::new(WEBKIT_INFO), "CFBundleVersion"),
        executable_path: std::env::current_exe()
            .map(|path| redact_home(&path.display().to_string()))
            .unwrap_or_default(),
        qualification_build: cfg!(feature = "qualification"),
    }
}
