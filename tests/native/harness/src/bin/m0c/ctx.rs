//! Shared runner context: repository root, identity, native helper and the
//! evidence root.

use std::path::PathBuf;

use serde_json::{Value, json};
use threadspace_harness::app::App;
use threadspace_harness::companion::Companion;
use threadspace_harness::evidence::sha256_file;
use threadspace_harness::identity::{Channel, Identity};
use threadspace_harness::native::Native;
use threadspace_harness::service;

pub struct Ctx {
    pub repo: PathBuf,
    pub id: Identity,
    pub native: Native,
}

impl Ctx {
    pub fn new(channel: &str) -> Result<Self, String> {
        let channel = Channel::parse(channel).ok_or("channel must be prod or dev")?;
        let repo = std::env::current_dir().map_err(|e| e.to_string())?;
        if !repo.join("docs/MILESTONES.md").exists() {
            return Err("run from the repository root".into());
        }
        let id = Identity::installed(channel).ok_or("identity paths unavailable")?;
        let native = Native::ensure(&repo)?;
        Ok(Self { repo, id, native })
    }

    pub fn evidence_root(&self) -> PathBuf {
        self.repo.join("evidence/M0C")
    }

    pub fn app(&self) -> App<'_> {
        App::new(&self.id)
    }

    pub fn companion(&self) -> Companion<'_> {
        Companion::new(&self.id)
    }

    /// Exact identity of what is installed and running.
    pub fn environment(&self) -> Value {
        let companion = self.companion();
        json!({
            "appIdentifier": self.id.app_identifier,
            "bundle": threadspace_relay::paths::redact_home(&self.id.bundle.display().to_string()),
            "executableSha256": sha256_file(&self.id.executable),
            "companionSha256": sha256_file(&self.id.companion_executable),
            "uiProcesses": self.app().processes(),
            "companionProcesses": companion.processes(),
            "companionLocatorIncarnation": companion.incarnation(),
            "service": service::status(&self.id),
            "diagnostics": companion.diagnostics().ok().map(|d| json!({
                "coreGeneration": d["coreGeneration"], "storeGeneration": d["storeGeneration"],
                "observationEnabled": d["observationEnabled"], "maintenancePhase": d["maintenancePhase"],
                "sqlite": { "version": d["sqlite"]["version"], "sourceId": d["sqlite"]["sourceId"], "journalMode": d["sqlite"]["journalMode"], "synchronous": d["sqlite"]["synchronous"] },
            })),
            "idleSeconds": self.native.idle_seconds(),
        })
    }
}
