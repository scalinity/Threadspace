//! Harness-only integration acquisition and directory ownership. The native
//! runner supplies the CLI closure; focused tests supply a disposable real
//! installer fixture, without native applications or owner configuration.

use serde_json::{Value, json};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

type IntegrationRunner<'a> = Box<dyn Fn(&str, &Path, &str) -> Result<Value, String> + 'a>;

/// Cleanup owns a newly created directory incarnation and, separately, the
/// successful installation record it acquired. Failure to install confers no
/// integration ownership. The runner is injectable so Drop is tested without
/// invoking native applications or owner configuration.
pub struct Scratch<'a> {
    integration: IntegrationRunner<'a>,
    pub dir: PathBuf,
    directory_identity: (u64, u64),
    acquired: Option<Value>,
}

impl<'a> Scratch<'a> {
    pub(super) fn with_runner(
        label: &str,
        integration: IntegrationRunner<'a>,
    ) -> Result<Self, String> {
        let dir = disposable(label)?;
        let metadata = std::fs::symlink_metadata(&dir).map_err(|error| error.to_string())?;
        Ok(Self {
            integration,
            dir,
            directory_identity: (metadata.dev(), metadata.ino()),
            acquired: None,
        })
    }

    fn owns_directory(&self) -> bool {
        std::fs::symlink_metadata(&self.dir).is_ok_and(|metadata| {
            metadata.is_dir() && (metadata.dev(), metadata.ino()) == self.directory_identity
        })
    }

    pub(super) fn install_session(&mut self) -> Result<Value, String> {
        self.install_scope("session")
    }

    pub(super) fn install_user(&mut self) -> Result<Value, String> {
        self.install_scope("user")
    }

    fn install_scope(&mut self, scope: &str) -> Result<Value, String> {
        if !self.owns_directory() || self.acquired.is_some() {
            return Err("integration acquisition has no fresh owned directory".into());
        }
        let config = self.dir.join("session-config");
        std::fs::create_dir_all(&config).map_err(|error| error.to_string())?;
        let before = (self.integration)("status", &config, scope)?;
        if before["ok"] != json!(true) || before["detail"]["installed"] != json!(false) {
            return Err(
                "integration acquisition refused: another installation is present or unverified"
                    .into(),
            );
        }
        let installed = (self.integration)("install", &config, scope)?;
        if installed["ok"] != json!(true) {
            return Err(format!("integration install failed: {installed}"));
        }
        let record = &installed["detail"];
        if record["scope"] != json!(scope) || record["identity"]["configDir"] != json!(config) {
            return Err("integration install returned no matching ownership record".into());
        }
        self.acquired = Some(record.clone());
        Ok(installed)
    }

    pub(super) fn reinstall(&mut self) -> Result<Value, String> {
        let expected = self.acquired.as_ref().ok_or("no acquired integration")?;
        if !self.owns_directory() {
            return Err("scratch directory incarnation changed; reinstall refused".into());
        }
        let config = self.dir.join("session-config");
        let scope = expected["scope"]
            .as_str()
            .ok_or("acquired record has no scope")?;
        let current = (self.integration)("status", &config, scope)?;
        if current["ok"] != json!(true) || &current["detail"]["record"] != expected {
            return Err("integration acquisition changed; reinstall refused".into());
        }
        let installed = (self.integration)("install", &config, scope)?;
        if installed["ok"] != json!(true)
            || installed["detail"]["scope"] != json!(scope)
            || installed["detail"]["identity"]["configDir"] != json!(config)
        {
            return Err("reinstall returned no matching ownership record".into());
        }
        self.acquired = Some(installed["detail"].clone());
        Ok(installed)
    }

    /// Used by explicit finish and Drop. A changed/replaced record is not this
    /// invocation's acquisition; leave it and the scratch proof for diagnosis.
    pub fn remove_integration(&mut self) -> Result<Value, String> {
        let Some(expected) = self.acquired.as_ref() else {
            return Err("this invocation did not acquire an integration".into());
        };
        if !self.owns_directory() {
            return Err("scratch directory incarnation changed; cleanup refused".into());
        }
        let config = self.dir.join("session-config");
        let scope = expected["scope"]
            .as_str()
            .ok_or("acquired record has no scope")?;
        let current = (self.integration)("status", &config, scope)?;
        if current["ok"] != json!(true) {
            return Err("integration ownership could not be reverified; cleanup refused".into());
        }
        if current["detail"]["installed"] == json!(false) {
            self.acquired = None;
            return Ok(current);
        }
        if &current["detail"]["record"] != expected {
            return Err("integration acquisition changed; cleanup refused".into());
        }
        let removed = (self.integration)("uninstall", &config, scope)?;
        if removed["ok"] == json!(true) && removed["detail"]["complete"] == json!(true) {
            self.acquired = None;
        }
        Ok(removed)
    }
}

impl Drop for Scratch<'_> {
    fn drop(&mut self) {
        if !self.owns_directory() {
            eprintln!("scratch cleanup refused: directory ownership changed");
            return;
        }
        if self.acquired.is_some() {
            let _ = self.remove_integration();
            if self.acquired.is_some() {
                eprintln!(
                    "scratch retained: integration cleanup is incomplete ({})",
                    self.dir.display()
                );
                return;
            }
        }
        // Recheck after the subprocess call, which may have yielded for a
        // while. Directory replacement never authorizes deleting its successor.
        if self.owns_directory() {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

pub fn disposable(label: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("ts-m2-{label}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

#[cfg(test)]
mod scratch_tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use threadspace_provider_claude::setup::{self, Scope, Target};

    fn fixture_bytes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(root: &Path, at: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(at).expect("fixture directory") {
                let path = entry.expect("fixture entry").path();
                let metadata = std::fs::symlink_metadata(&path).expect("fixture identity");
                assert!(!metadata.file_type().is_symlink());
                if metadata.is_dir() {
                    walk(root, &path, files);
                } else {
                    files.insert(
                        path.strip_prefix(root).expect("fixture path").to_owned(),
                        std::fs::read(path).expect("fixture bytes"),
                    );
                }
            }
        }
        let mut files = BTreeMap::new();
        walk(root, root, &mut files);
        files
    }

    #[test]
    fn f2_10_failed_session_activation_drop_cannot_uninstall_preexisting_user_install() {
        let owner = disposable("f2-owner-fixture").expect("disposable owner-like fixture");
        let owner_identity = std::fs::metadata(&owner).expect("fixture identity");
        let config = owner.join("config");
        let source = owner.join("source");
        std::fs::create_dir(&config).expect("fixture config");
        std::fs::create_dir_all(source.join("mod/.claude-plugin")).expect("fixture sources");
        std::fs::write(
            config.join("settings.json"),
            b"{\"env\":{\"KEEP\":\"original\"}}\n",
        )
        .expect("settings");
        std::fs::write(
            owner.join("unrelated-settings.json"),
            b"{\"untouched\":true}\n",
        )
        .expect("unrelated settings");
        std::fs::write(source.join("helper"), b"#!/bin/sh\nexit 0\n").expect("helper fixture");
        std::fs::write(source.join("mod/.claude-plugin/plugin.json"),
            br#"{"name":"fixture","userConfig":{"captureArgv":{"type":"string","multiple":true,"default":[]}}}"#).expect("mod fixture");
        let target = Target {
            config_dir: config,
            owned_dir: owner.join("owned"),
            agent_identifier: "ai.example.f2.agent".into(),
            helper_source: source.join("helper"),
            mod_source: source.join("mod"),
            scope: Scope::User,
        };
        setup::install(&target).expect("preexisting user integration");
        let before = fixture_bytes(&owner);
        let uninstall_calls = Cell::new(0);
        let runner = |op: &str, config: &Path, _scope: &str| -> Result<Value, String> {
            if op == "status" {
                // The acquisition observes absence, then a user installation
                // occupies the slot before install. The real installer below
                // must refuse, and that refusal must not create a lease.
                return Ok(json!({"ok":true,"detail":{"installed":false}}));
            }
            let mut session = target.clone();
            session.scope = Scope::Session;
            session.config_dir = config.to_owned();
            if op == "install" {
                let error = setup::install(&session)
                    .expect_err("preexisting user install refuses session acquisition");
                return Ok(json!({"ok":false,"detail":{"setupError":error}}));
            }
            assert_eq!(op, "uninstall");
            uninstall_calls.set(uninstall_calls.get() + 1);
            Ok(match setup::uninstall(&session) {
                Ok(report) => json!({"ok":true,"detail":report}),
                Err(error) => json!({"ok":false,"detail":{"setupError":error}}),
            })
        };
        let mut scratch =
            Scratch::with_runner("f2-failed-acquisition", Box::new(runner)).expect("scratch");
        let scratch_dir = scratch.dir.clone();
        assert!(scratch.install_session().is_err());
        assert!(scratch.acquired.is_none());
        drop(scratch);
        assert_eq!(
            uninstall_calls.get(),
            0,
            "a failed install never authorizes even an uninstall attempt"
        );
        assert_eq!(
            fixture_bytes(&owner),
            before,
            "settings, record, helper, mod, backups and unrelated bytes are unchanged"
        );
        if let Ok(directory) = std::env::var("THREADSPACE_F2_INVENTORY_DIR") {
            let hashes = |files: &BTreeMap<PathBuf, Vec<u8>>| {
                files
                    .iter()
                    .map(|(path, bytes)| {
                        let digest: String = Sha256::digest(bytes)
                            .iter()
                            .map(|byte| format!("{byte:02x}"))
                            .collect();
                        json!({"path":path,"bytes":bytes.len(),"sha256":digest})
                    })
                    .collect::<Vec<_>>()
            };
            let after = fixture_bytes(&owner);
            let inventory = json!({"case":"f2-10-failed-activation","before":hashes(&before),"after":hashes(&after),"byteEquality":before == after,"uninstallCalls":uninstall_calls.get()});
            std::fs::write(
                Path::new(&directory).join("f2-10-failed-activation.json"),
                serde_json::to_vec_pretty(&inventory).expect("inventory JSON"),
            )
            .expect("retain disposable fixture hashes");
        }
        assert!(
            !scratch_dir.exists(),
            "only the successfully created scratch directory was removed"
        );
        setup::uninstall(&target).expect("the test removes its own preexisting fixture");
        let owned_runner = |op: &str, config: &Path, scope: &str| -> Result<Value, String> {
            assert_eq!(scope, "user");
            let mut acquired_target = target.clone();
            acquired_target.config_dir = config.to_owned();
            let detail = match op {
                "status" => serde_json::to_value(setup::status(&acquired_target).expect("status")),
                "install" => {
                    serde_json::to_value(setup::install(&acquired_target).expect("install"))
                }
                "uninstall" => {
                    serde_json::to_value(setup::uninstall(&acquired_target).expect("remove"))
                }
                _ => panic!("unexpected integration operation"),
            }
            .expect("typed integration result");
            Ok(json!({"ok":true,"detail":detail}))
        };
        let mut acquired = Scratch::with_runner("f2-positive-acquisition", Box::new(owned_runner))
            .expect("owned positive fixture");
        let acquired_dir = acquired.dir.clone();
        let installed = acquired
            .install_user()
            .expect("successfully acquire user fixture");
        assert_eq!(acquired.acquired, Some(installed["detail"].clone()));
        acquired
            .reinstall()
            .expect("reinstall the recorded acquisition");
        let removed = acquired
            .remove_integration()
            .expect("remove the recorded acquisition");
        assert_eq!(removed["detail"]["complete"], json!(true));
        assert!(acquired.acquired.is_none());
        drop(acquired);
        assert!(!acquired_dir.exists());
        let current = std::fs::symlink_metadata(&owner).expect("fixture identity");
        assert_eq!(
            (current.dev(), current.ino()),
            (owner_identity.dev(), owner_identity.ino())
        );
        std::fs::remove_dir_all(owner).expect("remove the verified test-owned fixture");
    }
}
