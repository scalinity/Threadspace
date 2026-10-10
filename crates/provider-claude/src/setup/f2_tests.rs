// F2 focused integration ownership closure. These are child tests so they use
// the same disposable installer fixtures and actual production functions.
mod f2 {
    use super::*;
    use std::collections::BTreeMap;

    fn tree(root: &Path) -> BTreeMap<PathBuf, (Vec<u8>, u32)> {
        fn read(root: &Path, at: &Path, files: &mut BTreeMap<PathBuf, (Vec<u8>, u32)>) {
            for entry in fs::read_dir(at).expect("fixture directory") {
                let path = entry.expect("fixture entry").path();
                let metadata = fs::symlink_metadata(&path).expect("fixture metadata");
                assert!(
                    !metadata.file_type().is_symlink(),
                    "fixture has no borrowed links"
                );
                if metadata.is_dir() {
                    read(root, &path, files);
                } else {
                    files.insert(
                        path.strip_prefix(root)
                            .expect("owned fixture path")
                            .to_owned(),
                        (
                            fs::read(&path).expect("fixture bytes"),
                            metadata.permissions().mode() & 0o7777,
                        ),
                    );
                }
            }
        }
        let mut files = BTreeMap::new();
        read(root, root, &mut files);
        files
    }

    fn retain_inventory(
        case: &str,
        before: &BTreeMap<PathBuf, (Vec<u8>, u32)>,
        after: &BTreeMap<PathBuf, (Vec<u8>, u32)>,
    ) {
        let Ok(directory) = std::env::var("THREADSPACE_F2_INVENTORY_DIR") else {
            return;
        };
        let hashes = |files: &BTreeMap<PathBuf, (Vec<u8>, u32)>| {
            files.iter().map(|(path, (bytes, mode))| {
                serde_json::json!({"path":path,"bytes":bytes.len(),"mode":mode,"sha256":sha256_hex(bytes)})
            }).collect::<Vec<_>>()
        };
        let record = serde_json::json!({"case":case,"before":hashes(before),"after":hashes(after),"byteAndModeEquality":before == after});
        fs::write(
            Path::new(&directory).join(format!("{case}.json")),
            serde_json::to_vec_pretty(&record).expect("inventory JSON"),
        )
        .expect("retain disposable fixture hashes");
    }

    fn edit_hook(scratch: &Scratch, event: &str, edit: impl FnOnce(&mut Map)) -> Value {
        owner_edit(&scratch.settings(), |root| {
            let hook = event_groups(root, event)
                .last_mut()
                .and_then(Value::as_object_mut)
                .and_then(|group| group.get_mut("hooks"))
                .and_then(Value::as_array_mut)
                .and_then(|hooks| hooks.first_mut())
                .and_then(Value::as_object_mut)
                .expect("owned hook");
            edit(hook);
        });
        groups(&json(&scratch.settings()), event)
            .last()
            .expect("edited group")
            .clone()
    }

    fn preserved_resources(target: &Target, record: &InstallRecord) -> (Vec<u8>, Option<String>) {
        (
            fs::read(target.owned_dir.join("bin/threadspace-hook")).expect("helper"),
            owned::staged_sha256(&record.plugin_dir).expect("mod hash"),
        )
    }

    fn assert_partial(
        target: &Target,
        record: &InstallRecord,
        report: &UninstallReport,
        resources: &(Vec<u8>, Option<String>),
    ) {
        assert!(!report.complete);
        assert_eq!(
            report.uninstalled_at_ms, 0,
            "partial work is not reported as uninstalled"
        );
        assert_eq!(
            report.retained_helper_paths,
            [target.owned_dir.join("bin/threadspace-hook")]
        );
        assert_eq!(
            report.retained_mod_paths.as_slice(),
            std::slice::from_ref(&record.plugin_dir)
        );
        assert_eq!(
            report.retained_record,
            Some(target.owned_dir.join("record.json"))
        );
        assert_eq!(
            &preserved_resources(target, record),
            resources,
            "referenced executable/mod bytes survive"
        );
        let retained = owned::read_record(&target.owned_dir.join("record.json"))
            .expect("retained record")
            .expect("still installed");
        assert!(retained.partial_uninstall);
        assert_eq!(retained.identity, record.identity);
        assert_eq!(
            retained.applied_sha256, record.applied_sha256,
            "partial settings must not become an original-byte restore point"
        );
        assert_eq!(retained.owned_entries, report.conflicts);
    }

    #[test]
    fn f2_01_unchanged_install_reinstall_remove_is_byte_exact_for_ten_cycles() {
        ten_cycles_restore_the_original_bytes_exactly();
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::User);
        let record = install(&target).expect("install");
        assert_eq!(record.identity, Some(InstallIdentity::for_target(&target)));
        let report = uninstall(&target).expect("uninstall");
        assert!(report.complete && report.retained_record.is_none());
        assert!(report.retained_helper_paths.is_empty() && report.retained_mod_paths.is_empty());
        assert_eq!(
            fs::read(scratch.settings()).expect("settings"),
            COMPLEX.as_bytes()
        );
    }

    #[test]
    fn f2_02_changed_matcher_is_a_conflict_and_survives() {
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::User);
        let record = install(&target).expect("install");
        let resources = preserved_resources(&target, &record);
        owner_edit(&scratch.settings(), |root| {
            event_groups(root, "PreToolUse")
                .last_mut()
                .and_then(Value::as_object_mut)
                .expect("owned group")
                .insert("matcher", Value::string("Bash"));
        });
        let changed = groups(&json(&scratch.settings()), "PreToolUse")
            .last()
            .expect("group")
            .clone();
        let report = uninstall(&target).expect("partial uninstall");
        assert_eq!(
            report.conflicts,
            [OwnedEntry::Hook {
                event: "PreToolUse".into(),
                matcher: "*".into(),
                command: command(&target)
            }]
        );
        assert_eq!(
            groups(&json(&scratch.settings()), "PreToolUse").last(),
            Some(&changed)
        );
        assert_partial(&target, &record, &report, &resources);
    }

    #[test]
    fn f2_03_changed_timeout_is_a_conflict_and_survives() {
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::User);
        let record = install(&target).expect("install");
        let resources = preserved_resources(&target, &record);
        let changed = edit_hook(&scratch, "PreToolUse", |hook| {
            hook.insert("timeout", Value::Number("17".into()));
        });
        let report = uninstall(&target).expect("partial uninstall");
        assert_eq!(report.conflicts.len(), 1);
        assert_eq!(
            groups(&json(&scratch.settings()), "PreToolUse").last(),
            Some(&changed)
        );
        assert_partial(&target, &record, &report, &resources);
    }

    #[test]
    fn f2_04_changed_hook_type_is_a_conflict_and_survives() {
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::User);
        let record = install(&target).expect("install");
        let resources = preserved_resources(&target, &record);
        let changed = edit_hook(&scratch, "PreToolUse", |hook| {
            hook.insert("type", Value::string("prompt"));
        });
        let report = uninstall(&target).expect("partial uninstall");
        assert_eq!(report.conflicts.len(), 1);
        assert_eq!(
            groups(&json(&scratch.settings()), "PreToolUse").last(),
            Some(&changed)
        );
        assert_partial(&target, &record, &report, &resources);
    }

    #[test]
    fn f2_05_changed_command_retains_its_helper_and_record() {
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::User);
        let record = install(&target).expect("install");
        let resources = preserved_resources(&target, &record);
        let changed = edit_hook(&scratch, "Stop", |hook| {
            hook.insert(
                "command",
                Value::string(format!("{} --verbose", command(&target))),
            );
        });
        let report = uninstall(&target).expect("partial uninstall");
        assert_eq!(report.conflicts.len(), 1);
        assert_eq!(groups(&json(&scratch.settings()), "Stop"), [changed]);
        assert_partial(&target, &record, &report, &resources);
        let before = tree(&scratch.root);
        assert!(matches!(
            install(&target),
            Err(SetupError::UnresolvedConflicts { .. })
        ));
        assert_eq!(
            tree(&scratch.root),
            before,
            "reinstall cannot overwrite a retained conflicted helper/mod"
        );
    }

    #[test]
    fn f2_06_changed_plugin_reference_keeps_the_referenced_copy() {
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::User);
        let record = install(&target).expect("install");
        let resources = preserved_resources(&target, &record);
        let edited = format!(
            "/opt/plugins/a:/opt/plugins/b:{}/.",
            record.plugin_dir.display()
        );
        owner_edit(&scratch.settings(), |root| {
            root.get_mut("env")
                .and_then(Value::as_object_mut)
                .expect("env")
                .insert(PLUGIN_DIRS, Value::string(&edited));
        });
        let report = uninstall(&target).expect("partial uninstall");
        assert_eq!(
            report.conflicts,
            [OwnedEntry::PluginDir {
                path: path_text(&record.plugin_dir)
            }]
        );
        assert_eq!(
            plugin_dirs(&json(&scratch.settings())),
            Some(edited.as_str())
        );
        assert_partial(&target, &record, &report, &resources);
    }

    #[test]
    fn f2_07_foreign_siblings_and_their_order_survive_removal() {
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::User);
        install(&target).expect("install");
        let left = parsed(r#"{"type":"command","command":"foreign-left","timeout":3}"#);
        let right = parsed(r#"{"type":"command","command":"foreign-right"}"#);
        owner_edit(&scratch.settings(), |root| {
            let hooks = event_groups(root, "PreToolUse")
                .last_mut()
                .and_then(Value::as_object_mut)
                .and_then(|group| group.get_mut("hooks"))
                .and_then(Value::as_array_mut)
                .expect("hooks");
            hooks.insert(0, left.clone());
            hooks.push(right.clone());
        });
        let report = uninstall(&target).expect("uninstall");
        assert!(report.complete && report.conflicts.is_empty());
        let root = json(&scratch.settings());
        let groups_after = groups(&root, "PreToolUse");
        assert_eq!(&groups_after[..2], groups(&parsed(COMPLEX), "PreToolUse"));
        assert_eq!(
            groups_after[2].get("hooks").and_then(Value::as_array),
            Some(&vec![left, right])
        );
        assert_eq!(
            groups_after[2].get("matcher").and_then(Value::as_str),
            Some("*")
        );
        assert_eq!(root.get("mcpServers"), parsed(COMPLEX).get("mcpServers"));
    }

    #[test]
    fn f2_08_repeated_partial_uninstall_has_no_double_removal_or_foreign_loss() {
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::User);
        let record = install(&target).expect("install");
        let resources = preserved_resources(&target, &record);
        edit_hook(&scratch, "Stop", |hook| {
            hook.insert(
                "command",
                Value::string(format!("{} --verbose", command(&target))),
            );
        });
        let first = uninstall(&target).expect("partial uninstall");
        assert_eq!(first.conflicts.len(), 1);
        let settings = fs::read(scratch.settings()).expect("settings");
        let retained_record = fs::read(target.owned_dir.join("record.json")).expect("record");
        let second = uninstall(&target).expect("partial retry");
        assert!(second.removed.is_empty());
        assert_eq!(second.conflicts, first.conflicts);
        assert_eq!(fs::read(scratch.settings()).expect("settings"), settings);
        assert_eq!(
            fs::read(target.owned_dir.join("record.json")).expect("record"),
            retained_record
        );
        assert_partial(&target, &record, &second, &resources);
    }

    #[test]
    fn f2_09_wrong_scope_refuses_with_every_fixture_byte_unchanged() {
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::User);
        install(&target).expect("install");
        fs::write(
            scratch.root.join("unrelated-settings.json"),
            b"{\"keep\":true}\n",
        )
        .expect("unrelated");
        let before = tree(&scratch.root);
        assert!(matches!(
            uninstall(&scratch.target(Scope::Session)),
            Err(SetupError::ScopeMismatch {
                installed: Scope::User
            })
        ));
        assert_eq!(
            tree(&scratch.root),
            before,
            "settings, installation record, helper, mod, backups and unrelated config are byte-identical"
        );
        retain_inventory("f2-09-wrong-scope", &before, &tree(&scratch.root));
    }

    // F2-10 is the actual Scratch::drop regression in the native harness. Its
    // runner is injected, so the test invokes no native or owner application.

    #[test]
    fn f2_11_wrong_config_or_copied_owned_root_refuses_without_mutation() {
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::User);
        install(&target).expect("install");
        let mut other_config = target.clone();
        other_config.config_dir = scratch.root.join("other-config");
        fs::create_dir(&other_config.config_dir).expect("other config");
        fs::write(
            other_config.config_dir.join("settings.json"),
            b"{\"keep\":true}\n",
        )
        .expect("unrelated settings");
        let before = tree(&scratch.root);
        assert!(matches!(
            uninstall(&other_config),
            Err(SetupError::TargetMismatch { .. })
        ));
        assert_eq!(tree(&scratch.root), before);
        retain_inventory("f2-11-wrong-config", &before, &tree(&scratch.root));

        let mut other_root = target.clone();
        other_root.owned_dir = scratch.root.join("other-owned");
        fs::create_dir(&other_root.owned_dir).expect("other root");
        fs::copy(
            target.owned_dir.join("record.json"),
            other_root.owned_dir.join("record.json"),
        )
        .expect("copied record");
        let before = tree(&scratch.root);
        assert!(matches!(
            uninstall(&other_root),
            Err(SetupError::TargetMismatch { .. })
        ));
        assert_eq!(
            tree(&scratch.root),
            before,
            "a copied record is not authority over a different root"
        );
        retain_inventory("f2-11-wrong-owned-root", &before, &tree(&scratch.root));

        let path = target.owned_dir.join("record.json");
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).expect("record")).expect("JSON");
        legacy
            .as_object_mut()
            .expect("record object")
            .remove("identity");
        fs::write(&path, serde_json::to_vec(&legacy).expect("legacy JSON")).expect("legacy record");
        let before = tree(&scratch.root);
        assert!(matches!(
            uninstall(&target),
            Err(SetupError::InvalidRecord { .. })
        ));
        assert_eq!(
            tree(&scratch.root),
            before,
            "an old unqualified record cannot invent ownership"
        );
        retain_inventory("f2-11-legacy-identity", &before, &tree(&scratch.root));
    }

    #[test]
    fn f2_12_legitimately_resolved_conflict_finishes_on_retry() {
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::User);
        let record = install(&target).expect("install");
        edit_hook(&scratch, "Stop", |hook| {
            hook.insert(
                "command",
                Value::string(format!("{} --verbose", command(&target))),
            );
        });
        assert!(!uninstall(&target).expect("partial uninstall").complete);
        // The owner explicitly restores the recorded entry, resolving the
        // conflict. Retry removes that exact entry, not a changed foreign one.
        edit_hook(&scratch, "Stop", |hook| {
            hook.insert("command", Value::string(command(&target)));
        });
        let report = uninstall(&target).expect("resolved retry");
        assert!(report.complete && report.conflicts.is_empty());
        assert!(report.retained_record.is_none());
        assert!(!target.owned_dir.join("bin/threadspace-hook").exists());
        assert!(!record.plugin_dir.exists());
        assert!(!target.owned_dir.join("record.json").exists());
        assert_eq!(
            json(&scratch.settings()),
            parsed(COMPLEX),
            "foreign values and ordering survive both attempts"
        );
    }
}
