//! Replaces an installed qualification bundle with a freshly built one, in
//! the order M0B used: the old bootstrap unregisters its login item (which
//! terminates the old companion), the old bundle moves to a rollback copy
//! outside ~/Applications, the new bundle is copied in, and the new bootstrap
//! registers it. The new companion's incarnation and both executable hashes
//! are recorded.

use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};
use threadspace_harness::evidence::sha256_file;
use threadspace_harness::run::run;
use threadspace_harness::service;

use crate::ctx::Ctx;

pub fn install(ctx: &Ctx, built: &Path, rollback_root: &Path) -> Result<Value, String> {
    if !built.join("Contents/Info.plist").exists() {
        return Err(format!("{} is not an app bundle", built.display()));
    }
    let verify = run(
        "/usr/bin/codesign",
        &[
            "--verify",
            "--deep",
            "--strict",
            &built.display().to_string(),
        ],
        Duration::from_secs(60),
    );
    if !verify.ok {
        return Err(format!(
            "built bundle fails codesign verification: {}",
            verify.stderr.trim()
        ));
    }
    let companion = ctx.companion();
    let old_companion = companion.incarnation();
    let mut steps = Vec::new();
    if ctx.id.bundle.exists() {
        steps.push(json!({ "step": "unregister-old", "result": service::bootstrap(&ctx.id, "unregister") }));
        if let Some(old) = &old_companion {
            let exited = threadspace_harness::procs::wait_exit(old, Duration::from_secs(15));
            steps.push(json!({ "step": "old-companion-exit", "waitedMs": exited }));
        }
        ctx.app().stop_all();
        let backup = rollback_root.join(format!("{}-{}", ctx.id.app_identifier, crate::stamp()));
        std::fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
        let moved = run(
            "/bin/mv",
            &[
                &ctx.id.bundle.display().to_string(),
                &backup.display().to_string(),
            ],
            Duration::from_secs(60),
        );
        steps.push(json!({ "step": "move-old-bundle", "ok": moved.ok, "to": threadspace_relay::paths::redact_home(&backup.display().to_string()) }));
        if !moved.ok {
            return Err(format!(
                "could not move the old bundle: {}",
                moved.stderr.trim()
            ));
        }
    }
    let copied = run(
        "/usr/bin/ditto",
        &[
            &built.display().to_string(),
            &ctx.id.bundle.display().to_string(),
        ],
        Duration::from_secs(120),
    );
    if !copied.ok {
        return Err(format!("ditto failed: {}", copied.stderr.trim()));
    }
    let registered = service::bootstrap(&ctx.id, "register");
    steps.push(json!({ "step": "register-new", "result": registered }));
    let fresh = companion.wait_new_incarnation(old_companion.as_ref(), Duration::from_secs(30));
    let build_info = std::fs::read_to_string(ctx.repo.join(match ctx.id.channel {
        threadspace_harness::identity::Channel::Prod => {
            "apps/agent-macos/build/prod/build-info.json"
        }
        threadspace_harness::identity::Channel::Dev => "apps/agent-macos/build/dev/build-info.json",
    }))
    .ok()
    .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let ok = fresh.is_some();
    Ok(json!({
        "pass": ok,
        "steps": steps,
        "newCompanion": fresh.as_ref().map(|(inc, waited)| json!({ "incarnation": inc, "waitedMs": waited })),
        "installed": {
            "executableSha256": sha256_file(&ctx.id.executable),
            "companionSha256": sha256_file(&ctx.id.companion_executable),
            "companionBuildInfo": build_info,
        },
        "service": service::status(&ctx.id),
    }))
}
