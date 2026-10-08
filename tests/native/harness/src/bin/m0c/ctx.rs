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
    milestone: String,
}

impl Ctx {
    pub fn new(channel: &str) -> Result<Self, String> {
        let channel = Channel::parse(channel).ok_or("channel must be prod or dev")?;
        let repo = std::env::current_dir().map_err(|e| e.to_string())?;
        if !repo.join("docs/MILESTONES.md").exists() {
            return Err("run from the repository root".into());
        }
        // An invalid value is refused rather than defaulted, so a later
        // milestone's run never lands in accepted M0C evidence by mistake.
        let milestone = match std::env::var("THREADSPACE_EVIDENCE_MILESTONE") {
            Ok(milestone) if valid_milestone(&milestone) => milestone,
            Ok(milestone) => {
                return Err(format!(
                    "THREADSPACE_EVIDENCE_MILESTONE={milestone:?} must match ^M[0-9][A-Z0-9]*$"
                ));
            }
            Err(_) => "M0C".to_owned(),
        };
        let id = Identity::installed(channel).ok_or("identity paths unavailable")?;
        let native = Native::ensure(&repo)?;
        Ok(Self {
            repo,
            id,
            native,
            milestone,
        })
    }

    /// `evidence/<milestone>`: `THREADSPACE_EVIDENCE_MILESTONE` names a later
    /// milestone's run of an M0C runner; unset, it is `M0C`.
    pub fn evidence_root(&self) -> PathBuf {
        self.repo.join("evidence").join(&self.milestone)
    }

    /// Holds the machine-wide GUI automation lock for one short segment.
    pub fn gui(&self, label: &str) -> Result<threadspace_harness::idle::GuiLock, String> {
        threadspace_harness::idle::GuiLock::acquire(&format!("threadspace-m0c: {label}"))
            .map_err(|e| e.to_string())
    }

    pub fn channel_name(&self) -> &'static str {
        match self.id.channel {
            threadspace_harness::identity::Channel::Prod => "prod",
            threadspace_harness::identity::Channel::Dev => "dev",
        }
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

/// `^M[0-9][A-Z0-9]*$`, so the value is one directory name under `evidence/`.
fn valid_milestone(milestone: &str) -> bool {
    let bytes = milestone.as_bytes();
    bytes.len() >= 2
        && bytes[0] == b'M'
        && bytes[1].is_ascii_digit()
        && bytes[2..]
            .iter()
            .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::valid_milestone;

    #[test]
    fn milestone_names_one_evidence_directory() {
        for good in ["M0C", "M1", "M15", "M0A"] {
            assert!(valid_milestone(good), "{good}");
        }
        for bad in ["", "M", "m1", "MX", "M1a", "M1 ", "M1/c04", "../M1"] {
            assert!(!valid_milestone(bad), "{bad}");
        }
    }
}
