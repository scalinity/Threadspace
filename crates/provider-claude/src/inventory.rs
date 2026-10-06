//! `claude agents --json --all` (SPEC §4.4, §11.3): one bounded argv request
//! per call, parsed tolerantly (unknown fields ignored) but never trusted
//! beyond the fields identity needs. The request interval is recorded because
//! the inventory has no revision barrier: it is an interval observation.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use threadspace_contracts::route::{InventoryEvidence, InventoryRowEvidence};
use threadspace_surfaces_macos::exec::{BoundedCommand, run_bounded};

const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_ROWS: usize = 1024;
const MAX_SESSION_ID_CHARS: usize = 200;

/// One inventory row. Only `kind`, `pid` and `sessionId` take part in
/// identity; `name`, `cwd` and `startedAt` are labels/diagnostics and never
/// keys (SPEC §4.1, §4.8). `startedAt` is not kernel birth.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryRow {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub pid: Option<i64>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub waiting_for: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub started_at: Option<i64>,
    /// Background short job ID; never a full resumable session ID.
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

pub const KIND_INTERACTIVE: &str = "interactive";

impl InventoryRow {
    /// The full native session ID, if present and well-formed (opaque, but
    /// bounded and printable).
    pub fn full_session_id(&self) -> Option<&str> {
        let id = self.session_id.as_deref()?;
        let ok = !id.is_empty()
            && id.chars().count() <= MAX_SESSION_ID_CHARS
            && id.chars().all(|c| c.is_ascii_graphic());
        ok.then_some(id)
    }

    pub fn live_pid(&self) -> Option<i32> {
        self.pid
            .filter(|pid| *pid > 1 && *pid <= i64::from(i32::MAX))
            .map(|pid| pid as i32)
    }

    pub fn is_interactive(&self) -> bool {
        self.kind.as_deref() == Some(KIND_INTERACTIVE)
    }

    pub fn evidence(&self) -> InventoryRowEvidence {
        InventoryRowEvidence {
            pid: self.pid,
            session_id: self.session_id.clone(),
            kind: self.kind.clone(),
            status: self.status.clone(),
            waiting_for: self.waiting_for.clone(),
        }
    }
}

/// One completed inventory request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventorySnapshot {
    pub rows: Vec<InventoryRow>,
    pub request_started_ms: i64,
    pub request_ended_ms: i64,
    pub binary: String,
}

impl InventorySnapshot {
    pub fn rows_for_pid(&self, pid: i32) -> Vec<&InventoryRow> {
        self.rows
            .iter()
            .filter(|row| row.live_pid() == Some(pid))
            .collect()
    }

    pub fn rows_for_session(&self, session_id: &str) -> Vec<&InventoryRow> {
        self.rows
            .iter()
            .filter(|row| row.full_session_id() == Some(session_id))
            .collect()
    }

    /// Evidence about one target, with every row naming its PID or session.
    pub fn evidence_for(&self, pid: i32, session_id: &str) -> InventoryEvidence {
        InventoryEvidence {
            request_started_ms: self.request_started_ms,
            request_ended_ms: self.request_ended_ms,
            binary: self.binary.clone(),
            row_count: self.rows.len() as u32,
            pid_rows: self
                .rows_for_pid(pid)
                .iter()
                .map(|row| row.evidence())
                .collect(),
            session_rows: self
                .rows_for_session(session_id)
                .iter()
                .map(|row| row.evidence())
                .collect(),
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryError {
    Spawn(String),
    Failed {
        status: Option<i32>,
        timed_out: bool,
        stderr: String,
    },
    Truncated,
    Parse(String),
}

impl InventoryError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Spawn(_) => "INVENTORY_SPAWN_FAILED",
            Self::Failed {
                timed_out: true, ..
            } => "INVENTORY_TIMEOUT",
            Self::Failed { .. } => "INVENTORY_FAILED",
            Self::Truncated => "INVENTORY_TRUNCATED",
            Self::Parse(_) => "INVENTORY_PARSE_FAILED",
        }
    }
}

impl std::fmt::Display for InventoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(detail) => write!(f, "inventory spawn failed: {detail}"),
            Self::Failed {
                status,
                timed_out,
                stderr,
            } => write!(
                f,
                "inventory failed (status {status:?}, timed out {timed_out}): {stderr}"
            ),
            Self::Truncated => f.write_str("inventory output exceeded its bound"),
            Self::Parse(detail) => write!(f, "inventory output unparseable: {detail}"),
        }
    }
}

impl std::error::Error for InventoryError {}

/// Parses the JSON array. A non-array, oversized or malformed document is an
/// error; unknown fields are tolerated; non-object entries are dropped.
pub fn parse_rows(bytes: &[u8]) -> Result<Vec<InventoryRow>, InventoryError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| InventoryError::Parse(error.to_string()))?;
    let serde_json::Value::Array(items) = value else {
        return Err(InventoryError::Parse("not a JSON array".into()));
    };
    if items.len() > MAX_ROWS {
        return Err(InventoryError::Parse("too many rows".into()));
    }
    Ok(items
        .into_iter()
        .filter(serde_json::Value::is_object)
        .filter_map(|item| serde_json::from_value::<InventoryRow>(item).ok())
        .collect())
}

/// A source of inventory snapshots: the real CLI or a synthetic script.
pub trait Inventory {
    fn fetch(&self) -> Result<InventorySnapshot, InventoryError>;
}

/// The installed Claude CLI, invoked by absolute path with a cleared
/// environment. Threadspace's polling never creates model work; it also
/// suppresses the CLI's non-essential traffic and auto-update for these
/// administrative invocations only.
#[derive(Debug, Clone)]
pub struct ClaudeCli {
    pub binary: PathBuf,
    pub home: PathBuf,
    pub timeout: Duration,
    pub now_ms: fn() -> i64,
}

impl ClaudeCli {
    fn command(&self, timeout: Duration, max: usize) -> BoundedCommand {
        BoundedCommand::new(&self.binary, timeout, max)
            .env("HOME", &self.home)
            .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
            .env("DISABLE_AUTOUPDATER", "1")
    }

    /// `claude --version`, e.g. `2.1.291 (Claude Code)` → `2.1.291`.
    pub fn version(&self) -> Option<String> {
        let output =
            run_bounded(&self.command(Duration::from_secs(5), 4096).arg("--version")).ok()?;
        if !output.succeeded() {
            return None;
        }
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .next()
            .map(str::to_owned)
    }
}

impl Inventory for ClaudeCli {
    fn fetch(&self) -> Result<InventorySnapshot, InventoryError> {
        let started = (self.now_ms)();
        let output = run_bounded(
            &self
                .command(self.timeout, MAX_OUTPUT_BYTES)
                .arg("agents")
                .arg("--json")
                .arg("--all"),
        )
        .map_err(|error| InventoryError::Spawn(error.to_string()))?;
        let ended = (self.now_ms)();
        if !output.succeeded() {
            return Err(InventoryError::Failed {
                status: output.status,
                timed_out: output.timed_out,
                stderr: String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(240)
                    .collect(),
            });
        }
        if output.stdout_truncated {
            return Err(InventoryError::Truncated);
        }
        Ok(InventorySnapshot {
            rows: parse_rows(&output.stdout)?,
            request_started_ms: started,
            request_ended_ms: ended,
            binary: self.binary.display().to_string(),
        })
    }
}

/// The stable launcher the native installer repoints on update, resolved to
/// the versioned binary for each use, and the directory whose binaries are
/// qualified Claude CLI runtimes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeInstall {
    pub launcher: PathBuf,
    pub binary: PathBuf,
    pub versions_dir: PathBuf,
}

impl ClaudeInstall {
    pub fn resolve(launcher: &Path) -> Option<Self> {
        let binary = std::fs::canonicalize(launcher).ok()?;
        let versions_dir = binary.parent()?.to_path_buf();
        Some(Self {
            launcher: launcher.to_path_buf(),
            binary,
            versions_dir,
        })
    }

    /// The qualified direct CLI runtime rule (SPEC §4.5 step 3): a binary
    /// that lives in the installer's versions directory. A process-name
    /// substring is never the rule.
    pub fn qualifies(&self, executable_path: &str) -> bool {
        Path::new(executable_path).parent() == Some(self.versions_dir.as_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_installed_shape_and_tolerates_unknown_fields() {
        let rows = parse_rows(
            br#"[
              {"pid":33982,"cwd":"/x","kind":"interactive","startedAt":1791257980265,
               "sessionId":"c1225c17-0000-4000-8000-000000000001","name":"a","status":"idle"},
              {"pid":7894,"kind":"interactive","sessionId":"575562e7-0000-4000-8000-000000000002",
               "status":"waiting","waitingFor":"input needed","futureField":{"x":1}},
              {"kind":"background","id":"ab12","state":"working"},
              "not an object"
            ]"#,
        )
        .expect("parse");
        assert_eq!(rows.len(), 3);
        assert!(rows[0].is_interactive());
        assert_eq!(rows[0].live_pid(), Some(33982));
        assert_eq!(rows[1].waiting_for.as_deref(), Some("input needed"));
        assert_eq!(
            rows[2].full_session_id(),
            None,
            "a short job id is not a session id"
        );
        assert!(!rows[2].is_interactive());
    }

    #[test]
    fn rejects_non_arrays_and_malformed_ids() {
        assert!(parse_rows(br#"{"rows":[]}"#).is_err());
        assert!(parse_rows(b"not json").is_err());
        let rows = parse_rows(br#"[{"kind":"interactive","pid":1,"sessionId":"has space"}]"#)
            .expect("parse");
        assert_eq!(rows[0].full_session_id(), None);
        assert_eq!(
            rows[0].live_pid(),
            None,
            "pid 1 is launchd, never a provider"
        );
    }

    #[test]
    fn qualification_is_the_versions_directory_not_a_name() {
        let install = ClaudeInstall {
            launcher: "/h/.local/bin/claude".into(),
            binary: "/h/.local/share/claude/versions/2.1.291".into(),
            versions_dir: "/h/.local/share/claude/versions".into(),
        };
        assert!(install.qualifies("/h/.local/share/claude/versions/2.1.290"));
        assert!(!install.qualifies("/usr/local/bin/claude"));
        assert!(!install.qualifies("/tmp/claude-versions/claude"));
    }
}
