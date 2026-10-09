//! Reversible Claude integration setup (SPEC §19.2). It stages the capture
//! helper and the qualified observer mod into a Threadspace-owned directory,
//! then either adds passive command hooks and the mod's plugin directory to
//! the user's `settings.json` (user scope) or writes owned per-launch
//! settings and leaves the Claude configuration untouched (session scope).
//!
//! Every edit is planned first, backed up, written only while the file still
//! hashes as read, and recorded with the exact entries Threadspace owns. A
//! reinstall adds nothing; an uninstall restores the original bytes while the
//! file is exactly as applied, and otherwise removes only the owned entries
//! that still match, reporting the rest as conflicts. `WorktreeCreate` and
//! `WorktreeRemove` are never registered, and no hook makes a decision.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub mod ordered_json;
mod owned;
mod settings;

use ordered_json::{Map, Value};
use owned::{Layout, ModCopy, read_optional, sha256_hex};
use settings::PLUGIN_DIRS;

/// The hook events installed, with their matcher: every tool for tool
/// events, everything otherwise.
pub const HOOK_EVENTS: &[(&str, &str)] = &[
    ("SessionStart", ""),
    ("UserPromptSubmit", ""),
    ("Stop", ""),
    ("SubagentStop", ""),
    ("SubagentStart", ""),
    ("PreToolUse", "*"),
    ("PostToolUse", "*"),
    ("PostToolUseFailure", "*"),
    ("PermissionRequest", "*"),
    ("SessionEnd", ""),
    ("Notification", ""),
    ("StopFailure", ""),
    ("PermissionDenied", ""),
    ("PreCompact", ""),
    ("PostCompact", ""),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    /// Hooks and the plugin directory in `<config_dir>/settings.json`.
    User,
    /// An owned `--settings` file and `CLAUDE_CODE_PLUGIN_DIRS` value for a
    /// single launch; `config_dir` is never read or written.
    Session,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub config_dir: PathBuf,
    pub owned_dir: PathBuf,
    pub agent_identifier: String,
    pub helper_source: PathBuf,
    pub mod_source: PathBuf,
    pub scope: Scope,
}

/// One Threadspace-owned configuration entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OwnedEntry {
    Hook {
        event: String,
        matcher: String,
        command: String,
    },
    PluginDir {
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: PathBuf,
    /// SHA-256 of the current content (for the mod copy, of its file set);
    /// `None` when absent.
    pub before_sha256: Option<String>,
    pub after_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub files: Vec<FileChange>,
    pub owned_entries: Vec<OwnedEntry>,
    pub rollback: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRecord {
    pub scope: Scope,
    /// The settings file holding the owned hooks.
    pub config_path: PathBuf,
    /// SHA-256 of the bytes uninstall restores; `None` when Threadspace
    /// created the file.
    pub original_sha256: Option<String>,
    pub applied_sha256: String,
    pub hook_command: String,
    pub plugin_dir: PathBuf,
    /// The original bytes' file name under `backups/`.
    pub backup: Option<String>,
    pub owned_entries: Vec<OwnedEntry>,
    /// Containers the edit created, removed again once empty.
    pub created: Vec<String>,
    pub installed_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallReport {
    pub scope: Scope,
    pub config_path: PathBuf,
    /// The settings file holds its original bytes again (or is gone, when
    /// Threadspace created it).
    pub restored_original: bool,
    pub removed: Vec<OwnedEntry>,
    /// Owned entries no longer found as installed; left untouched.
    pub conflicts: Vec<OwnedEntry>,
    pub settings_sha256: Option<String>,
    pub removed_files: Vec<PathBuf>,
    pub uninstalled_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationState {
    pub installed: bool,
    pub record: Option<InstallRecord>,
    pub settings_sha256: Option<String>,
    /// The settings file still holds exactly the bytes Threadspace wrote.
    pub settings_unchanged: bool,
    /// Owned entries no longer found as installed.
    pub missing: Vec<OwnedEntry>,
    pub helper_present: bool,
    pub plugin_dir_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "code",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase"
)]
pub enum SetupError {
    InvalidTarget {
        reason: String,
    },
    ConfigDirMissing {
        path: PathBuf,
    },
    MissingSource {
        path: PathBuf,
    },
    InvalidMod {
        path: PathBuf,
        reason: String,
    },
    /// Merging could not be lossless, so the file was not written.
    InvalidSettings {
        path: PathBuf,
        reason: String,
    },
    InvalidRecord {
        path: PathBuf,
        reason: String,
    },
    /// The file's hash moved between the read and the write.
    ChangedDuringInstall {
        path: PathBuf,
    },
    NotInstalled,
    ScopeMismatch {
        installed: Scope,
    },
    Io {
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for SetupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTarget { reason } => {
                write!(formatter, "invalid integration target: {reason}")
            }
            Self::ConfigDirMissing { path } => write!(
                formatter,
                "Claude configuration directory {} does not exist",
                path.display()
            ),
            Self::MissingSource { path } => write!(
                formatter,
                "bundled integration source {} is missing",
                path.display()
            ),
            Self::InvalidMod { path, reason } => write!(
                formatter,
                "observer mod {} cannot be staged: {reason}",
                path.display()
            ),
            Self::InvalidSettings { path, reason } => write!(
                formatter,
                "{} cannot be merged losslessly and was not written: {reason}",
                path.display()
            ),
            Self::InvalidRecord { path, reason } => write!(
                formatter,
                "install record {} is unreadable: {reason}",
                path.display()
            ),
            Self::ChangedDuringInstall { path } => write!(
                formatter,
                "{} changed while it was being edited and was not written",
                path.display()
            ),
            Self::NotInstalled => formatter.write_str("the Claude integration is not installed"),
            Self::ScopeMismatch { installed } => write!(
                formatter,
                "the Claude integration is installed with {} scope; uninstall it first",
                match installed {
                    Scope::User => "user",
                    Scope::Session => "session",
                }
            ),
            Self::Io { path, message } => write!(formatter, "{}: {message}", path.display()),
        }
    }
}

impl std::error::Error for SetupError {}

pub fn plan(target: &Target) -> Result<Plan, SetupError> {
    let prepared = prepare(target)?;
    let layout = &prepared.layout;
    let mut files = vec![FileChange {
        path: prepared.settings_path.clone(),
        before_sha256: prepared.before.as_deref().map(sha256_hex),
        after_sha256: sha256_hex(&prepared.after),
    }];
    if target.scope == Scope::Session {
        files.push(FileChange {
            path: layout.session_env.clone(),
            before_sha256: read_optional(&layout.session_env)?
                .as_deref()
                .map(sha256_hex),
            after_sha256: sha256_hex(&activate_env(&path_text(&prepared.plugin_dir))),
        });
    }
    files.push(FileChange {
        path: layout.helper.clone(),
        before_sha256: read_optional(&layout.helper)?.as_deref().map(sha256_hex),
        after_sha256: sha256_hex(&prepared.helper),
    });
    files.push(FileChange {
        path: prepared.plugin_dir.clone(),
        before_sha256: owned::staged_sha256(&prepared.plugin_dir)?,
        after_sha256: prepared.mod_copy.sha256.clone(),
    });
    let settings = prepared.settings_path.display();
    let owned_files = format!("{}, {}", layout.bin.display(), layout.observer.display());
    let rollback = match target.scope {
        Scope::User => vec![
            match &prepared.base.backup {
                Some(backup) => format!(
                    "Uninstall restores {settings} byte-for-byte from backups/{backup} while its SHA-256 is still {}.",
                    prepared.base.applied_sha256
                ),
                None => format!(
                    "Uninstall deletes {settings}, which this install creates, while its SHA-256 is still {}.",
                    prepared.base.applied_sha256
                ),
            },
            "Otherwise it removes only the owned entries that still match exactly, and reports edited ones as conflicts without touching them.".into(),
            format!("It then removes {owned_files} and record.json; backups/ is kept."),
        ],
        Scope::Session => vec![
            format!(
                "Launch Claude with the variable in {} and `--settings {settings}`; {} is not modified.",
                layout.session_env.display(),
                target.config_dir.display()
            ),
            format!(
                "Uninstall removes {}, {owned_files} and record.json; backups/ is kept.",
                layout.session.display()
            ),
        ],
    };
    Ok(Plan {
        files,
        owned_entries: prepared.entries,
        rollback,
    })
}

/// Idempotent: a second install over an unchanged integration writes no
/// settings and keeps one owned set.
pub fn install(target: &Target) -> Result<InstallRecord, SetupError> {
    let prepared = prepare(target)?;
    let layout = &prepared.layout;
    layout.create()?;
    owned::write_atomic(&layout.helper, &prepared.helper, 0o755)?;
    prepared.mod_copy.stage(&prepared.plugin_dir)?;
    let base = &prepared.base;
    match target.scope {
        Scope::User => {
            if let (true, Some(before), Some(backup)) =
                (base.new_backup, &prepared.before, &base.backup)
            {
                owned::write_atomic(&layout.backups.join(backup), before, 0o600)?;
            }
            if prepared.before.as_deref() != Some(prepared.after.as_slice()) {
                let expected = prepared.before.as_deref().map(sha256_hex);
                owned::replace_checked(
                    &prepared.settings_path,
                    expected.as_deref(),
                    Some(&prepared.after),
                )?;
            }
        }
        Scope::Session => {
            owned::create_private_dir(&layout.session)?;
            owned::write_atomic(&layout.session_settings, &prepared.after, 0o600)?;
            let env = activate_env(&path_text(&prepared.plugin_dir));
            owned::write_atomic(&layout.session_env, &env, 0o600)?;
        }
    }
    let mut created = prepared
        .previous
        .map(|record| record.created)
        .unwrap_or_default();
    for path in prepared.created {
        if !created.contains(&path) {
            created.push(path);
        }
    }
    let record = InstallRecord {
        scope: target.scope,
        config_path: prepared.settings_path,
        original_sha256: base.original_sha256.clone(),
        applied_sha256: base.applied_sha256.clone(),
        hook_command: prepared.hook_command,
        plugin_dir: prepared.plugin_dir,
        backup: base.backup.clone(),
        owned_entries: prepared.entries,
        created,
        installed_at_ms: now_ms(),
    };
    owned::write_json(&layout.record, &record)?;
    // Earlier mod copies are unreferenced once the settings are written.
    owned::remove_others(&layout.observer, &record.plugin_dir)?;
    Ok(record)
}

pub fn uninstall(target: &Target) -> Result<UninstallReport, SetupError> {
    validate(target)?;
    let layout = Layout::new(&target.owned_dir);
    let record = owned::read_record(&layout.record)?.ok_or(SetupError::NotInstalled)?;
    let mut report = UninstallReport {
        scope: record.scope,
        config_path: record.config_path.clone(),
        restored_original: false,
        removed: Vec::new(),
        conflicts: Vec::new(),
        settings_sha256: None,
        removed_files: Vec::new(),
        uninstalled_at_ms: 0,
    };
    match record.scope {
        Scope::User => restore_settings(&layout, &record, &mut report)?,
        Scope::Session => report.removed = record.owned_entries.clone(),
    }
    // Owned files go only once no settings reference them.
    for path in [
        &layout.session,
        &layout.bin,
        &layout.observer,
        &layout.record,
    ] {
        if owned::remove_owned(path)? {
            report.removed_files.push(path.clone());
        }
    }
    report.uninstalled_at_ms = now_ms();
    owned::create_private_dir(&layout.backups)?;
    owned::write_json(&layout.backups.join("uninstall-report.json"), &report)?;
    Ok(report)
}

pub fn status(target: &Target) -> Result<IntegrationState, SetupError> {
    validate(target)?;
    let layout = Layout::new(&target.owned_dir);
    let Some(record) = owned::read_record(&layout.record)? else {
        return Ok(IntegrationState {
            installed: false,
            record: None,
            settings_sha256: None,
            settings_unchanged: false,
            missing: Vec::new(),
            helper_present: layout.helper.is_file(),
            plugin_dir_present: false,
        });
    };
    let current = read_optional(&record.config_path)?;
    let (in_file, elsewhere): (Vec<OwnedEntry>, Vec<OwnedEntry>) = record
        .owned_entries
        .iter()
        .cloned()
        .partition(|entry| in_settings(record.scope, entry));
    let mut missing = match &current {
        Some(bytes) => {
            let mut root = parse_root(&record.config_path, bytes)?;
            settings::remove(&mut root, &in_file, &[]).conflicts
        }
        None => in_file,
    };
    // Session scope keeps the plugin directory in activate.env.
    let env = read_optional(&layout.session_env)?;
    for entry in elsewhere {
        if let OwnedEntry::PluginDir { path } = &entry
            && env.as_deref() != Some(activate_env(path).as_slice())
        {
            missing.push(entry);
        }
    }
    let settings_sha256 = current.as_deref().map(sha256_hex);
    Ok(IntegrationState {
        installed: true,
        settings_unchanged: settings_sha256.as_deref() == Some(record.applied_sha256.as_str()),
        settings_sha256,
        missing,
        helper_present: layout.helper.is_file(),
        plugin_dir_present: record.plugin_dir.is_dir(),
        record: Some(record),
    })
}

/// Everything an install would do, computed from one read of each file.
struct Prepared {
    layout: Layout,
    previous: Option<InstallRecord>,
    helper: Vec<u8>,
    mod_copy: ModCopy,
    hook_command: String,
    plugin_dir: PathBuf,
    entries: Vec<OwnedEntry>,
    settings_path: PathBuf,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
    created: Vec<String>,
    base: Base,
}

/// What the record says about the settings file after this install.
struct Base {
    original_sha256: Option<String>,
    backup: Option<String>,
    applied_sha256: String,
    /// The bytes being replaced are the original and need backing up.
    new_backup: bool,
}

fn prepare(target: &Target) -> Result<Prepared, SetupError> {
    validate(target)?;
    let layout = Layout::new(&target.owned_dir);
    let previous = owned::read_record(&layout.record)?;
    if let Some(record) = &previous
        && record.scope != target.scope
    {
        return Err(SetupError::ScopeMismatch {
            installed: record.scope,
        });
    }
    let helper =
        read_optional(&target.helper_source)?.ok_or_else(|| SetupError::MissingSource {
            path: target.helper_source.clone(),
        })?;
    let helper_path = path_text(&layout.helper);
    let hook_command = format!(
        "{} hook --agent {}",
        shell_quote(&helper_path),
        target.agent_identifier
    );
    let capture_argv = [
        helper_path,
        "mod-batch".into(),
        "--agent".into(),
        target.agent_identifier.clone(),
    ];
    let mod_copy = ModCopy::read(&target.mod_source, &capture_argv)?;
    let plugin_dir = layout.observer.join(mod_copy.dir_name());
    let entries = owned_entries(&hook_command, &path_text(&plugin_dir));

    let (settings_path, before, root) = match target.scope {
        Scope::User => {
            if !target.config_dir.is_dir() {
                return Err(SetupError::ConfigDirMissing {
                    path: target.config_dir.clone(),
                });
            }
            let path = target.config_dir.join("settings.json");
            let before = read_optional(&path)?;
            let root = match &before {
                Some(bytes) => parse_root(&path, bytes)?,
                None => Map::new(),
            };
            (path, before, root)
        }
        Scope::Session => (
            layout.session_settings.clone(),
            read_optional(&layout.session_settings)?,
            Map::new(),
        ),
    };
    let mut edited = root.clone();
    let in_file: Vec<OwnedEntry> = entries
        .iter()
        .filter(|entry| in_settings(target.scope, entry))
        .cloned()
        .collect();
    let stale_prefix = format!("{}/", path_text(&layout.observer));
    let created = settings::add(&mut edited, &in_file, &stale_prefix).map_err(|reason| {
        SetupError::InvalidSettings {
            path: settings_path.clone(),
            reason,
        }
    })?;
    let after = match &before {
        // Nothing to add: keep the bytes exactly, formatting included.
        Some(bytes) if edited == root => bytes.clone(),
        _ => pretty(&settings_path, edited)?,
    };
    let base = base(target.scope, previous.as_ref(), before.as_deref(), &after);
    Ok(Prepared {
        layout,
        previous,
        helper,
        mod_copy,
        hook_command,
        plugin_dir,
        entries,
        settings_path,
        before,
        after,
        created,
        base,
    })
}

fn base(
    scope: Scope,
    previous: Option<&InstallRecord>,
    before: Option<&[u8]>,
    after: &[u8],
) -> Base {
    let applied_sha256 = sha256_hex(after);
    let before_sha256 = before.map(sha256_hex);
    match (scope, previous) {
        (Scope::Session, _) => Base {
            original_sha256: None,
            backup: None,
            applied_sha256,
            new_backup: false,
        },
        // Nothing to write: the previous record still describes the file.
        (Scope::User, Some(record)) if before == Some(after) => Base {
            original_sha256: record.original_sha256.clone(),
            backup: record.backup.clone(),
            applied_sha256: record.applied_sha256.clone(),
            new_backup: false,
        },
        // Rewriting Threadspace's own applied bytes: the recorded original
        // is still the one to restore.
        (Scope::User, Some(record))
            if before_sha256.as_deref() == Some(record.applied_sha256.as_str()) =>
        {
            Base {
                original_sha256: record.original_sha256.clone(),
                backup: record.backup.clone(),
                applied_sha256,
                new_backup: false,
            }
        }
        (Scope::User, _) => Base {
            backup: before_sha256.as_deref().map(owned::backup_name),
            original_sha256: before_sha256,
            applied_sha256,
            new_backup: before.is_some(),
        },
    }
}

fn restore_settings(
    layout: &Layout,
    record: &InstallRecord,
    report: &mut UninstallReport,
) -> Result<(), SetupError> {
    let path = &record.config_path;
    let current = read_optional(path)?;
    let current_sha256 = current.as_deref().map(sha256_hex);
    if current_sha256.as_deref() == Some(record.applied_sha256.as_str())
        && let Some(original) = restorable_original(layout, record)?
    {
        owned::replace_checked(path, current_sha256.as_deref(), original.as_deref())?;
        report.restored_original = true;
        report.removed = record.owned_entries.clone();
        report.settings_sha256 = record.original_sha256.clone();
        return Ok(());
    }
    let Some(bytes) = current else {
        // The file is gone, and every owned entry with it.
        report.conflicts = record.owned_entries.clone();
        return Ok(());
    };
    let mut root = parse_root(path, &bytes)?;
    let removal = settings::remove(&mut root, &record.owned_entries, &record.created);
    let after = if removal.changed {
        let after = pretty(path, root)?;
        owned::replace_checked(path, current_sha256.as_deref(), Some(&after))?;
        after
    } else {
        bytes
    };
    report.settings_sha256 = Some(sha256_hex(&after));
    report.removed = removal.removed;
    report.conflicts = removal.conflicts;
    Ok(())
}

/// The bytes to restore while the file is exactly as applied: `Some(None)`
/// when Threadspace created the file. `None` when the backup is missing or
/// altered, or itself holds owned entries (it was taken over an earlier
/// install the owner had edited); entry removal applies instead.
fn restorable_original(
    layout: &Layout,
    record: &InstallRecord,
) -> Result<Option<Option<Vec<u8>>>, SetupError> {
    let (Some(original_sha256), Some(backup)) = (&record.original_sha256, &record.backup) else {
        return Ok(record.original_sha256.is_none().then_some(None));
    };
    let Some(bytes) = read_optional(&layout.backups.join(backup))? else {
        return Ok(None);
    };
    if sha256_hex(&bytes) != *original_sha256 {
        return Ok(None);
    }
    let clean = match Value::parse(&bytes) {
        Ok(Value::Object(mut root)) => settings::remove(&mut root, &record.owned_entries, &[])
            .removed
            .is_empty(),
        _ => false,
    };
    Ok(clean.then_some(Some(bytes)))
}

fn validate(target: &Target) -> Result<(), SetupError> {
    let identifier = &target.agent_identifier;
    if identifier.is_empty()
        || !identifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
    {
        return Err(SetupError::InvalidTarget {
            reason: "the agent identifier must be reverse-DNS characters".into(),
        });
    }
    // The owned paths go into a `:`-separated list and a line-based file.
    match target.owned_dir.to_str() {
        Some(dir) if target.owned_dir.is_absolute() && !dir.contains([':', '\n']) => Ok(()),
        _ => Err(SetupError::InvalidTarget {
            reason: "the owned directory must be an absolute UTF-8 path without `:` or newlines"
                .into(),
        }),
    }
}

fn owned_entries(hook_command: &str, plugin_dir: &str) -> Vec<OwnedEntry> {
    HOOK_EVENTS
        .iter()
        .map(|(event, matcher)| OwnedEntry::Hook {
            event: (*event).into(),
            matcher: (*matcher).into(),
            command: hook_command.into(),
        })
        .chain([OwnedEntry::PluginDir {
            path: plugin_dir.into(),
        }])
        .collect()
}

/// Session scope passes the plugin directory in the environment instead.
fn in_settings(scope: Scope, entry: &OwnedEntry) -> bool {
    scope == Scope::User || matches!(entry, OwnedEntry::Hook { .. })
}

fn activate_env(plugin_dir: &str) -> Vec<u8> {
    format!("{PLUGIN_DIRS}={plugin_dir}\n").into_bytes()
}

fn parse_root(path: &Path, bytes: &[u8]) -> Result<Map, SetupError> {
    let invalid = |reason: String| SetupError::InvalidSettings {
        path: path.to_owned(),
        reason,
    };
    match Value::parse(bytes) {
        Ok(Value::Object(root)) => Ok(root),
        Ok(_) => Err(invalid("the root is not an object".into())),
        Err(error) => Err(invalid(error.to_string())),
    }
}

fn pretty(path: &Path, root: Map) -> Result<Vec<u8>, SetupError> {
    Value::Object(root)
        .to_pretty()
        .map_err(|error| SetupError::InvalidSettings {
            path: path.to_owned(),
            reason: error.to_string(),
        })
}

/// Validated paths are UTF-8.
fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// POSIX single-quoting: the owned path contains spaces.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU32, Ordering};

    const AGENT: &str = "ai.example.threadspace.agent";

    const MOD_FILES: &[(&str, &str)] = &[
        (
            ".claude-plugin/plugin.json",
            "{\n  \"name\": \"threadspace-observer\",\n  \"userConfig\": {\n    \"captureArgv\": {\n      \"type\": \"string\",\n      \"multiple\": true,\n      \"default\": []\n    }\n  }\n}\n",
        ),
        (".claude-plugin/types/index.d.ts", "export {}\n"),
        ("hooks/hooks.json", "{ \"modules\": [\"./register.ts\"] }\n"),
        ("hooks/register.ts", "export default () => {}\n"),
        ("hooks/tsconfig.json", "{}\n"),
        ("tests/register.test.ts", "test\n"),
        ("tsconfig.json", "{}\n"),
    ];

    /// Foreign configuration a real profile carries, written its own way
    /// (four-space indent): hooks with matchers on several events, MCP
    /// servers, permissions, an existing plugin directory list, unknown
    /// keys, unicode and numbers.
    const COMPLEX: &str = r#"{
    "model": "opus",
    "hooks": {
        "SessionStart": [
            { "matcher": "startup", "hooks": [ { "type": "command", "command": "echo start" } ] }
        ],
        "PreToolUse": [
            { "matcher": "Bash", "hooks": [ { "type": "command", "command": "guard.sh", "timeout": 5 }, { "type": "command", "command": "log.sh" } ] },
            { "matcher": "Edit|Write", "hooks": [ { "type": "command", "command": "fmt.sh" } ] }
        ],
        "WorktreeCreate": [
            { "hooks": [ { "type": "command", "command": "make-worktree.sh" } ] }
        ]
    },
    "mcpServers": { "files": { "command": "npx", "args": ["-y", "server"], "env": { "ROOT": "~/w" } } },
    "permissions": { "allow": ["Bash(git status)"], "deny": [], "defaultMode": "acceptEdits" },
    "env": { "EDITOR": "vim", "CLAUDE_CODE_PLUGIN_DIRS": "/opt/plugins/a:/opt/plugins/b", "MAX_THINKING_TOKENS": "1024" },
    "x-unknown": { "nested": [1.50, 1e3, -0, 123456789012345678901234567890], "flag": null },
    "statusLine": { "type": "command", "command": "echo \u2713 ünïcödé 日本" },
    "cleanupPeriodDays": 30
}
"#;

    /// A disposable root: a Claude configuration directory, an owned
    /// directory whose path has a space, and the bundled sources.
    struct Scratch {
        root: PathBuf,
    }

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let root = std::env::temp_dir().join(format!(
                "ts-setup-{}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
                now_ms()
            ));
            fs::create_dir_all(root.join("config")).expect("config dir");
            fs::create_dir_all(root.join("bundle")).expect("bundle dir");
            fs::write(root.join("bundle/threadspace-hook"), b"#!/bin/sh\nexit 0\n")
                .expect("helper");
            for (relative, text) in MOD_FILES {
                let path = root.join("bundle/provider-mod").join(relative);
                fs::create_dir_all(path.parent().expect("parent")).expect("mod dir");
                fs::write(path, text).expect("mod file");
            }
            Self { root }
        }

        fn with_settings(text: &str) -> Self {
            let scratch = Self::new();
            fs::write(scratch.settings(), text).expect("settings");
            scratch
        }

        fn target(&self, scope: Scope) -> Target {
            Target {
                config_dir: self.root.join("config"),
                owned_dir: self
                    .root
                    .join("Application Support/ai.example.threadspace/integrations/claude"),
                agent_identifier: AGENT.into(),
                helper_source: self.root.join("bundle/threadspace-hook"),
                mod_source: self.root.join("bundle/provider-mod"),
                scope,
            }
        }

        fn settings(&self) -> PathBuf {
            self.root.join("config/settings.json")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn command(target: &Target) -> String {
        format!(
            "'{}' hook --agent {AGENT}",
            target.owned_dir.join("bin/threadspace-hook").display()
        )
    }

    fn json(path: &Path) -> Value {
        Value::parse(&fs::read(path).expect("read")).expect("json")
    }

    fn parsed(text: &str) -> Value {
        Value::parse(text.as_bytes()).expect("json")
    }

    fn groups<'a>(root: &'a Value, event: &str) -> &'a [Value] {
        root.get("hooks")
            .and_then(|hooks| hooks.get(event))
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice)
    }

    fn owned_group(matcher: &str, command: &str) -> Value {
        parsed(&format!(
            r#"{{"matcher":{},"hooks":[{{"type":"command","command":{}}}]}}"#,
            serde_json::Value::from(matcher),
            serde_json::Value::from(command)
        ))
    }

    /// Hooks carrying `command` across every event.
    fn count_command(root: &Value, command: &str) -> usize {
        root.get("hooks")
            .and_then(Value::as_object)
            .map(|hooks| {
                hooks
                    .keys()
                    .flat_map(|event| groups(root, event))
                    .filter_map(|group| group.get("hooks").and_then(Value::as_array))
                    .flatten()
                    .filter(|hook| hook.get("command").and_then(Value::as_str) == Some(command))
                    .count()
            })
            .unwrap_or(0)
    }

    fn plugin_dirs(root: &Value) -> Option<&str> {
        root.get("env")
            .and_then(|env| env.get(PLUGIN_DIRS))
            .and_then(Value::as_str)
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).expect("stat").permissions().mode() & 0o7777
    }

    fn listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("list")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    /// Edits the settings the way the owner would, saving pretty JSON.
    fn owner_edit(path: &Path, edit: impl FnOnce(&mut Map)) {
        let mut root = parse_root(path, &fs::read(path).expect("read")).expect("root");
        edit(&mut root);
        fs::write(path, Value::Object(root).to_pretty().expect("print")).expect("write");
    }

    fn event_groups<'a>(root: &'a mut Map, event: &str) -> &'a mut Vec<Value> {
        root.get_mut("hooks")
            .and_then(Value::as_object_mut)
            .and_then(|hooks| hooks.get_mut(event))
            .and_then(Value::as_array_mut)
            .expect("event groups")
    }

    #[test]
    fn install_into_empty_config_then_uninstall_removes_the_created_file() {
        let scratch = Scratch::new();
        let target = scratch.target(Scope::User);
        let record = install(&target).expect("install");
        let command = command(&target);
        assert_eq!(record.hook_command, command);

        let root = json(&scratch.settings());
        let events: Vec<&str> = root
            .get("hooks")
            .and_then(Value::as_object)
            .expect("hooks")
            .keys()
            .collect();
        assert_eq!(
            events,
            HOOK_EVENTS
                .iter()
                .map(|(event, _)| *event)
                .collect::<Vec<_>>()
        );
        for (event, matcher) in HOOK_EVENTS {
            assert_eq!(
                groups(&root, event),
                [owned_group(matcher, &command)],
                "{event}"
            );
        }
        assert_eq!(plugin_dirs(&root), record.plugin_dir.to_str());
        assert_eq!(mode(&scratch.settings()), 0o600);
        assert_eq!(
            (record.original_sha256.as_ref(), record.backup.as_ref()),
            (None, None)
        );

        let helper = target.owned_dir.join("bin/threadspace-hook");
        assert_eq!(fs::read(&helper).expect("helper"), b"#!/bin/sh\nexit 0\n");
        assert_eq!(mode(&helper), 0o755);
        assert_eq!(mode(&target.owned_dir.join("record.json")), 0o600);

        // The mod copy has no tests or type-checking files, and its capture
        // command is the owned helper.
        let copy = &record.plugin_dir;
        assert_eq!(
            copy.parent(),
            Some(target.owned_dir.join("observer").as_path())
        );
        assert_eq!(copy.file_name().map(|name| name.len()), Some(16));
        assert_eq!(listing(copy), [".claude-plugin", "hooks"]);
        assert_eq!(listing(&copy.join(".claude-plugin")), ["plugin.json"]);
        assert_eq!(listing(&copy.join("hooks")), ["hooks.json", "register.ts"]);
        let manifest = json(&copy.join(".claude-plugin/plugin.json"));
        let argv = manifest
            .get("userConfig")
            .and_then(|config| config.get("captureArgv"))
            .and_then(|capture| capture.get("default"))
            .expect("default");
        let helper_text = helper.to_str().expect("utf-8");
        assert_eq!(
            argv,
            &parsed(&serde_json::json!([helper_text, "mod-batch", "--agent", AGENT]).to_string())
        );

        let report = uninstall(&target).expect("uninstall");
        assert!(report.restored_original && report.conflicts.is_empty());
        assert!(
            listing(&target.config_dir).is_empty(),
            "the created file is gone"
        );
        assert_eq!(listing(&target.owned_dir), ["backups"]);
    }

    #[test]
    fn install_into_complex_settings_keeps_foreign_configuration() {
        let scratch = Scratch::with_settings(COMPLEX);
        fs::set_permissions(scratch.settings(), fs::Permissions::from_mode(0o644)).expect("chmod");
        let target = scratch.target(Scope::User);
        let original = parsed(COMPLEX);
        let record = install(&target).expect("install");
        let command = command(&target);
        let root = json(&scratch.settings());

        let keys = |value: &Value| -> Vec<String> {
            value
                .as_object()
                .expect("object")
                .keys()
                .map(str::to_owned)
                .collect()
        };
        assert_eq!(keys(&root), keys(&original));
        for key in [
            "model",
            "mcpServers",
            "permissions",
            "x-unknown",
            "statusLine",
            "cleanupPeriodDays",
        ] {
            assert_eq!(root.get(key), original.get(key), "{key}");
        }
        // Foreign groups keep their place; ours follow them.
        let mut start = groups(&original, "SessionStart").to_vec();
        start.push(owned_group("", &command));
        assert_eq!(groups(&root, "SessionStart"), start);
        let mut tools = groups(&original, "PreToolUse").to_vec();
        tools.push(owned_group("*", &command));
        assert_eq!(groups(&root, "PreToolUse"), tools);
        assert_eq!(
            groups(&root, "WorktreeCreate"),
            groups(&original, "WorktreeCreate")
        );
        assert_eq!(count_command(&root, &command), HOOK_EVENTS.len());

        let env = root.get("env").expect("env");
        assert_eq!(keys(env), ["EDITOR", PLUGIN_DIRS, "MAX_THINKING_TOKENS"]);
        assert_eq!(
            plugin_dirs(&root),
            Some(
                format!(
                    "/opt/plugins/a:/opt/plugins/b:{}",
                    record.plugin_dir.display()
                )
                .as_str()
            )
        );
        let text = fs::read_to_string(scratch.settings()).expect("text");
        for written in [
            "1.50",
            "1e3",
            "-0",
            "123456789012345678901234567890",
            "✓ ünïcödé 日本",
        ] {
            assert!(text.contains(written), "{written}");
        }
        assert_eq!(mode(&scratch.settings()), 0o644, "the file keeps its mode");
        assert_eq!(
            listing(&target.config_dir),
            ["settings.json"],
            "no temporary files remain"
        );

        uninstall(&target).expect("uninstall");
        assert_eq!(
            fs::read(scratch.settings()).expect("read"),
            COMPLEX.as_bytes()
        );
        assert_eq!(mode(&scratch.settings()), 0o644);
    }

    #[test]
    fn install_twice_keeps_one_owned_set() {
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::User);
        let first = install(&target).expect("install");
        let applied = fs::read(scratch.settings()).expect("read");
        let second = install(&target).expect("reinstall");
        assert_eq!(fs::read(scratch.settings()).expect("read"), applied);

        let root = json(&scratch.settings());
        assert_eq!(count_command(&root, &command(&target)), HOOK_EVENTS.len());
        let ours = second.plugin_dir.to_str().expect("utf-8");
        let dirs = plugin_dirs(&root).expect("plugin dirs");
        assert_eq!(dirs.split(':').filter(|dir| *dir == ours).count(), 1);
        assert_eq!(
            (
                &second.original_sha256,
                &second.backup,
                &second.applied_sha256
            ),
            (&first.original_sha256, &first.backup, &first.applied_sha256)
        );
        assert_eq!(listing(&target.owned_dir.join("observer")).len(), 1);
        assert_eq!(listing(&target.owned_dir.join("backups")).len(), 1);
    }

    #[test]
    fn ten_cycles_restore_the_original_bytes_exactly() {
        for original in [None, Some(COMPLEX)] {
            let scratch = original.map_or_else(Scratch::new, Scratch::with_settings);
            let target = scratch.target(Scope::User);
            for cycle in 0..10 {
                install(&target).expect("install");
                install(&target).expect("reinstall");
                let report = uninstall(&target).expect("uninstall");
                assert!(report.restored_original, "cycle {cycle}");
                assert!(report.conflicts.is_empty(), "cycle {cycle}");
                assert_eq!(
                    read_optional(&scratch.settings()).expect("read"),
                    original.map(|text| text.as_bytes().to_vec()),
                    "cycle {cycle}"
                );
                assert_eq!(listing(&target.owned_dir), ["backups"], "cycle {cycle}");
            }
            let mut kept = vec!["uninstall-report.json".to_owned()];
            if let Some(text) = original {
                kept.insert(0, owned::backup_name(&sha256_hex(text.as_bytes())));
            }
            assert_eq!(listing(&target.owned_dir.join("backups")), kept);
        }
    }

    #[test]
    fn uninstall_keeps_owner_edit_as_conflict() {
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::User);
        let record = install(&target).expect("install");
        let command = command(&target);
        let edited = format!("{command} --verbose");
        owner_edit(&scratch.settings(), |root| {
            let group = event_groups(root, "SessionStart")
                .last_mut()
                .and_then(Value::as_object_mut)
                .expect("owned group");
            let hook = group
                .get_mut("hooks")
                .and_then(Value::as_array_mut)
                .and_then(|hooks| hooks.first_mut())
                .and_then(Value::as_object_mut)
                .expect("owned hook");
            hook.insert("command", Value::string(edited.as_str()));
        });

        let report = uninstall(&target).expect("uninstall");
        assert!(!report.restored_original);
        let session_start = OwnedEntry::Hook {
            event: "SessionStart".into(),
            matcher: String::new(),
            command: command.clone(),
        };
        assert_eq!(report.conflicts, [session_start]);
        assert_eq!(report.removed.len(), record.owned_entries.len() - 1);

        let root = json(&scratch.settings());
        let original = parsed(COMPLEX);
        let mut start = groups(&original, "SessionStart").to_vec();
        start.push(owned_group("", &edited));
        assert_eq!(
            groups(&root, "SessionStart"),
            start,
            "the owner's edit stays"
        );
        assert_eq!(count_command(&root, &command), 0);
        assert_eq!(groups(&root, "PreToolUse"), groups(&original, "PreToolUse"));
        let events: Vec<&str> = root
            .get("hooks")
            .and_then(Value::as_object)
            .expect("hooks")
            .keys()
            .collect();
        assert_eq!(events, ["SessionStart", "PreToolUse", "WorktreeCreate"]);
        assert_eq!(plugin_dirs(&root), Some("/opt/plugins/a:/opt/plugins/b"));
        assert_eq!(listing(&target.owned_dir), ["backups"]);
    }

    #[test]
    fn uninstall_after_foreign_edit_removes_only_owned_entries() {
        let scratch = Scratch::new();
        let target = scratch.target(Scope::User);
        install(&target).expect("install");
        owner_edit(&scratch.settings(), |root| {
            event_groups(root, "PreToolUse").push(parsed(
                r#"{"matcher":"Bash","hooks":[{"type":"command","command":"theirs.sh"}]}"#,
            ));
            root.get_mut("env")
                .and_then(Value::as_object_mut)
                .expect("env")
                .insert("FOO", Value::string("1"));
            root.insert("model", Value::string("sonnet"));
        });

        let report = uninstall(&target).expect("uninstall");
        assert!(!report.restored_original && report.conflicts.is_empty());
        assert_eq!(
            json(&scratch.settings()),
            parsed(
                r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"theirs.sh"}]}]},"env":{"FOO":"1"},"model":"sonnet"}"#
            )
        );
    }

    #[test]
    fn settings_changed_between_read_and_write_are_refused() {
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::User);
        owned::BEFORE_REREAD.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(|path: &Path| {
                let mut text = fs::read_to_string(path).expect("read");
                text.push('\n');
                fs::write(path, text).expect("racing edit");
            }));
        });
        let error = install(&target).expect_err("refused");
        assert!(
            matches!(error, SetupError::ChangedDuringInstall { .. }),
            "{error}"
        );
        assert_eq!(
            fs::read_to_string(scratch.settings()).expect("read"),
            format!("{COMPLEX}\n"),
            "the racing edit survives"
        );
        assert!(!status(&target).expect("status").installed);
    }

    #[test]
    fn backups_are_owner_only_under_the_owned_dir() {
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::User);
        let record = install(&target).expect("install");
        let backups = target.owned_dir.join("backups");
        let backup = backups.join(record.backup.expect("backup"));
        assert_eq!(fs::read(&backup).expect("backup"), COMPLEX.as_bytes());
        assert_eq!(mode(&backup), 0o600);
        assert_eq!(mode(&backups), 0o700);
        assert_eq!(record.original_sha256, Some(sha256_hex(COMPLEX.as_bytes())));
    }

    #[test]
    fn symlinked_settings_are_written_through_to_their_target() {
        let scratch = Scratch::new();
        let dotfiles = scratch.root.join("dotfiles/settings.json");
        fs::create_dir_all(dotfiles.parent().expect("parent")).expect("dotfiles");
        fs::write(&dotfiles, COMPLEX).expect("settings");
        std::os::unix::fs::symlink(&dotfiles, scratch.settings()).expect("link");
        let target = scratch.target(Scope::User);

        install(&target).expect("install");
        let link = fs::symlink_metadata(scratch.settings()).expect("lstat");
        assert!(
            link.file_type().is_symlink(),
            "the link survives the install"
        );
        assert_eq!(
            count_command(&json(&dotfiles), &command(&target)),
            HOOK_EVENTS.len()
        );

        assert!(uninstall(&target).expect("uninstall").restored_original);
        let link = fs::symlink_metadata(scratch.settings()).expect("lstat");
        assert!(
            link.file_type().is_symlink(),
            "the link survives the uninstall"
        );
        assert_eq!(fs::read(&dotfiles).expect("read"), COMPLEX.as_bytes());
    }

    #[test]
    fn worktree_hooks_are_never_installed() {
        let reserved = ["WorktreeCreate", "WorktreeRemove"];
        assert!(
            HOOK_EVENTS
                .iter()
                .all(|(event, _)| !reserved.contains(event))
        );
        let scratch = Scratch::new();
        install(&scratch.target(Scope::User)).expect("install");
        let root = json(&scratch.settings());
        for event in reserved {
            assert!(groups(&root, event).is_empty(), "{event}");
        }

        let scratch = Scratch::with_settings(COMPLEX);
        install(&scratch.target(Scope::User)).expect("install");
        let root = json(&scratch.settings());
        assert_eq!(
            groups(&root, "WorktreeCreate"),
            groups(&parsed(COMPLEX), "WorktreeCreate")
        );
        assert!(groups(&root, "WorktreeRemove").is_empty());
    }

    #[test]
    fn session_scope_writes_only_owned_files() {
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::Session);
        let config_before = (listing(&target.config_dir), mode(&scratch.settings()));
        let record = install(&target).expect("install");
        let session = target.owned_dir.join("session");
        assert_eq!(record.config_path, session.join("settings.json"));
        assert_eq!(record.backup, None);

        let root = json(&session.join("settings.json"));
        assert_eq!(
            root.as_object().expect("object").keys().collect::<Vec<_>>(),
            ["hooks"]
        );
        for (event, matcher) in HOOK_EVENTS {
            assert_eq!(
                groups(&root, event),
                [owned_group(matcher, &command(&target))]
            );
        }
        assert_eq!(
            fs::read_to_string(session.join("activate.env")).expect("env"),
            format!("CLAUDE_CODE_PLUGIN_DIRS={}\n", record.plugin_dir.display())
        );
        let state = status(&target).expect("status");
        assert!(state.installed && state.settings_unchanged && state.missing.is_empty());
        assert!(matches!(
            install(&scratch.target(Scope::User)),
            Err(SetupError::ScopeMismatch {
                installed: Scope::Session
            })
        ));

        uninstall(&target).expect("uninstall");
        assert_eq!(listing(&target.owned_dir), ["backups"]);
        assert_eq!(
            (listing(&target.config_dir), mode(&scratch.settings())),
            config_before
        );
        assert_eq!(
            fs::read(scratch.settings()).expect("read"),
            COMPLEX.as_bytes()
        );
    }

    #[test]
    fn a_new_mod_replaces_the_old_copy_and_still_restores_the_original() {
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::User);
        let first = install(&target).expect("install");
        fs::write(
            target.mod_source.join("hooks/register.ts"),
            "export default () => 1\n",
        )
        .expect("new mod");
        let second = install(&target).expect("upgrade");
        assert_ne!(first.plugin_dir, second.plugin_dir);
        assert_eq!(
            plugin_dirs(&json(&scratch.settings())),
            Some(
                format!(
                    "/opt/plugins/a:/opt/plugins/b:{}",
                    second.plugin_dir.display()
                )
                .as_str()
            )
        );
        assert_eq!(
            listing(&target.owned_dir.join("observer")),
            [second
                .plugin_dir
                .file_name()
                .expect("name")
                .to_string_lossy()]
        );
        assert_eq!(second.backup, first.backup);

        assert!(uninstall(&target).expect("uninstall").restored_original);
        assert_eq!(
            fs::read(scratch.settings()).expect("read"),
            COMPLEX.as_bytes()
        );
    }

    #[test]
    fn settings_that_cannot_be_merged_are_left_alone() {
        for text in [
            "[]",
            r#"{"hooks": []}"#,
            r#"{"env": {"CLAUDE_CODE_PLUGIN_DIRS": 1}}"#,
            r#"{"a": 1, "a": 2}"#,
            "{",
        ] {
            let scratch = Scratch::with_settings(text);
            let target = scratch.target(Scope::User);
            let error = install(&target).expect_err(text);
            assert!(
                matches!(error, SetupError::InvalidSettings { .. }),
                "{text}: {error}"
            );
            assert_eq!(fs::read_to_string(scratch.settings()).expect("read"), text);
            assert!(!target.owned_dir.exists(), "{text}");
        }
    }

    #[test]
    fn plan_writes_nothing_and_matches_the_install() {
        let scratch = Scratch::with_settings(COMPLEX);
        let target = scratch.target(Scope::User);
        let planned = plan(&target).expect("plan");
        assert!(!target.owned_dir.exists());
        assert_eq!(
            fs::read(scratch.settings()).expect("read"),
            COMPLEX.as_bytes()
        );
        let settings = &planned.files[0];
        assert_eq!(settings.path, scratch.settings());
        assert_eq!(settings.before_sha256, Some(sha256_hex(COMPLEX.as_bytes())));
        assert_eq!(planned.owned_entries.len(), HOOK_EVENTS.len() + 1);
        let backup = owned::backup_name(&sha256_hex(COMPLEX.as_bytes()));
        assert!(
            planned.rollback[0].contains(&backup),
            "{}",
            planned.rollback[0]
        );

        let record = install(&target).expect("install");
        assert_eq!(record.applied_sha256, settings.after_sha256);
        assert_eq!(record.owned_entries, planned.owned_entries);
        let again = plan(&target).expect("plan");
        assert!(
            again
                .files
                .iter()
                .all(|file| file.before_sha256.as_ref() == Some(&file.after_sha256))
        );
    }

    #[test]
    fn status_reports_owned_entries_no_longer_installed() {
        let scratch = Scratch::new();
        let target = scratch.target(Scope::User);
        assert!(!status(&target).expect("status").installed);
        install(&target).expect("install");
        let state = status(&target).expect("status");
        assert!(state.installed && state.settings_unchanged && state.missing.is_empty());
        assert!(state.helper_present && state.plugin_dir_present);

        owner_edit(&scratch.settings(), |root| {
            root.get_mut("hooks")
                .and_then(Value::as_object_mut)
                .expect("hooks")
                .remove("Stop");
        });
        let state = status(&target).expect("status");
        assert!(!state.settings_unchanged);
        assert_eq!(
            state.missing,
            [OwnedEntry::Hook {
                event: "Stop".into(),
                matcher: String::new(),
                command: command(&target),
            }]
        );
    }

    #[test]
    fn hook_command_quotes_the_owned_path() {
        assert_eq!(shell_quote("/a b/it's"), r"'/a b/it'\''s'");
        let scratch = Scratch::new();
        let target = scratch.target(Scope::User);
        let record = install(&target).expect("install");
        assert!(record.hook_command.starts_with('\''));
        assert!(
            record
                .hook_command
                .ends_with(&format!("/bin/threadspace-hook' hook --agent {AGENT}"))
        );
    }

    #[test]
    fn invalid_targets_and_missing_sources_are_refused() {
        let scratch = Scratch::new();
        let mut target = scratch.target(Scope::User);
        target.agent_identifier = "a b".into();
        assert!(matches!(
            plan(&target),
            Err(SetupError::InvalidTarget { .. })
        ));
        let mut target = scratch.target(Scope::User);
        target.owned_dir = PathBuf::from("relative");
        assert!(matches!(
            install(&target),
            Err(SetupError::InvalidTarget { .. })
        ));
        let mut target = scratch.target(Scope::User);
        target.helper_source = scratch.root.join("absent");
        assert!(matches!(
            install(&target),
            Err(SetupError::MissingSource { .. })
        ));
        let mut target = scratch.target(Scope::User);
        target.config_dir = scratch.root.join("absent");
        assert!(matches!(
            install(&target),
            Err(SetupError::ConfigDirMissing { .. })
        ));
        assert_eq!(
            uninstall(&scratch.target(Scope::User)),
            Err(SetupError::NotInstalled)
        );
    }

    #[test]
    fn the_real_observer_mod_copies_without_tests() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/provider-mod");
        let argv = [
            "/owned/bin/threadspace-hook".to_owned(),
            "mod-batch".into(),
            "--agent".into(),
            AGENT.into(),
        ];
        let copy = ModCopy::read(&source, &argv).expect("read");
        let scratch = Scratch::new();
        let target = scratch.root.join("observer").join(copy.dir_name());
        fs::create_dir_all(target.parent().expect("parent")).expect("observer");
        copy.stage(&target).expect("stage");
        assert!(!target.join("tests").exists());
        assert!(target.join("hooks/register.ts").is_file());
        assert_eq!(
            fs::read(target.join("hooks/hooks.json")).expect("copy"),
            fs::read(source.join("hooks/hooks.json")).expect("source")
        );
        let manifest = json(&target.join(".claude-plugin/plugin.json"));
        let source_manifest = json(&source.join(".claude-plugin/plugin.json"));
        assert_eq!(
            manifest
                .as_object()
                .expect("object")
                .keys()
                .collect::<Vec<_>>(),
            source_manifest
                .as_object()
                .expect("object")
                .keys()
                .collect::<Vec<_>>()
        );
        assert_eq!(
            manifest
                .get("userConfig")
                .and_then(|config| config.get("captureArgv"))
                .and_then(|capture| capture.get("default")),
            Some(&parsed(&serde_json::json!(argv).to_string()))
        );
        assert_eq!(
            owned::staged_sha256(&target).expect("hash"),
            Some(copy.sha256.clone())
        );
    }
}
