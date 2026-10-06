//! The companion's single-writer SQLite journal (SPEC §9).
//!
//! One companion process owns writes under `WriterLock`. The connection runs
//! WAL, `synchronous=FULL` and `foreign_keys=ON`, and refuses to open unless
//! the linked engine is exactly SQLite 3.53.4 (docs/decisions/D-0001). A
//! receipt is produced only after `COMMIT` returns.

mod identity;
mod lock;
mod projection;
mod schema;

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use sha2::{Digest, Sha256};
use threadspace_contracts::cursor::format_cursor;
use threadspace_contracts::diagnostics::SqliteDiagnostics;
use threadspace_contracts::projection::{FleetSnapshot, NotificationState, ProjectionPatch};
use threadspace_contracts::ui::{CommandReceipt, ReceiptStatus};
use uuid::Uuid;

pub use identity::{
    ActivationChange, ApplyOutcome, BindingRow, DiscoveryApplication, LiveExecutionRow,
    ObservedSessionRecord, ProcessRecord, RouteTargetRow, SurfaceRecord,
};
pub use lock::{LockError, WriterLock};
pub use schema::SCHEMA_VERSION;

pub const REQUIRED_SQLITE_VERSION: &str = "3.53.4";
pub const REQUIRED_SQLITE_SOURCE_ID: &str =
    "2026-07-24 19:02:57 bf7c7f30031888f4e796e429ab3978879485813aaca6f641c7b33e4e09459bcc";

const FIXTURE_PROVIDER: &str = "synthetic";
const FIXTURE_PROFILE: &str = "m0a-fixture";
const FIXTURE_NATIVE_SESSION: &str = "m0a-fixture-session-1";
const FIXTURE_NATIVE_TURN: &str = "m0a-fixture-turn-1";
const SOURCE_FIXTURE: &str = "m0a.fixture";
const SOURCE_OWNER: &str = "owner";
const SOURCE_NOTIFICATIONS: &str = "companion.notifications";

#[derive(Debug)]
pub enum JournalError {
    EngineMismatch { version: String, source_id: String },
    SchemaTooNew { found: u32 },
    MigrationChecksum { id: u32 },
    PragmaRejected { pragma: &'static str, value: String },
    NotFound { entity: &'static str, id: String },
    Conflict { detail: String },
    Invalid { detail: String },
    Sqlite(rusqlite::Error),
}

impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EngineMismatch { version, source_id } => write!(
                f,
                "linked SQLite {version} ({source_id}) is not the required {REQUIRED_SQLITE_VERSION}"
            ),
            Self::SchemaTooNew { found } => {
                write!(
                    f,
                    "store schema {found} is newer than supported {SCHEMA_VERSION}"
                )
            }
            Self::MigrationChecksum { id } => write!(f, "migration {id} checksum differs"),
            Self::PragmaRejected { pragma, value } => write!(f, "PRAGMA {pragma} reported {value}"),
            Self::NotFound { entity, id } => write!(f, "{entity} {id} not found"),
            Self::Conflict { detail } | Self::Invalid { detail } => f.write_str(detail),
            Self::Sqlite(error) => write!(f, "sqlite: {error}"),
        }
    }
}

impl std::error::Error for JournalError {}

impl From<rusqlite::Error> for JournalError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

/// Entities touched by one committed transaction, for building a patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub cursor: i64,
    pub session_ids: Vec<String>,
    pub attention_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutcome {
    pub receipt: CommandReceipt,
    /// `None` when the same command was already committed (nothing new to stream).
    pub change: Option<Change>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionTarget {
    pub attention_id: String,
    pub session_id: String,
    pub outstanding: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationIntent {
    pub request_id: String,
    pub attention_id: String,
    pub session_id: String,
    pub title: String,
    pub body: String,
}

#[cfg(feature = "qualification")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaisedAttention {
    pub change: Change,
    pub intent: NotificationIntent,
}

pub struct Journal {
    conn: Connection,
    path: PathBuf,
    store_generation: String,
    endpoint_id: String,
    source_epoch: String,
}

impl std::fmt::Debug for Journal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Journal")
            .field("path", &self.path)
            .field("store_generation", &self.store_generation)
            .finish_non_exhaustive()
    }
}

/// Reads the engine actually linked into this process.
pub fn verify_engine(conn: &Connection) -> Result<(String, String), JournalError> {
    let (version, source_id): (String, String) =
        conn.query_row("SELECT sqlite_version(), sqlite_source_id()", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
    if version != REQUIRED_SQLITE_VERSION || source_id != REQUIRED_SQLITE_SOURCE_ID {
        return Err(JournalError::EngineMismatch { version, source_id });
    }
    Ok((version, source_id))
}

fn enum_text<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => text,
        _ => String::new(),
    }
}

fn fingerprint(payload: &serde_json::Value) -> String {
    let digest = Sha256::digest(payload.to_string().as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl Journal {
    /// Opens (creating if needed) the journal at `path`. The caller must hold
    /// the `WriterLock` for the containing store directory.
    pub fn open(path: &Path, source_epoch: &str, now_ms: i64) -> Result<Self, JournalError> {
        let mut conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        verify_engine(&conn)?;
        let mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        if mode != "wal" {
            return Err(JournalError::PragmaRejected {
                pragma: "journal_mode",
                value: mode,
            });
        }
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let synchronous: i64 = conn.query_row("PRAGMA synchronous", [], |row| row.get(0))?;
        let foreign_keys: i64 = conn.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
        if synchronous != 2 {
            return Err(JournalError::PragmaRejected {
                pragma: "synchronous",
                value: synchronous.to_string(),
            });
        }
        if foreign_keys != 1 {
            return Err(JournalError::PragmaRejected {
                pragma: "foreign_keys",
                value: foreign_keys.to_string(),
            });
        }
        schema::migrate(&mut conn, now_ms)?;

        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let store_generation = meta_get_or_insert(&tx, "store_generation")?;
        let endpoint_id = meta_get_or_insert(&tx, "endpoint_id")?;
        tx.commit()?;

        let mut journal = Self {
            conn,
            path: path.to_path_buf(),
            store_generation,
            endpoint_id,
            source_epoch: source_epoch.to_owned(),
        };
        journal.ensure_fixture(now_ms)?;
        Ok(journal)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn store_generation(&self) -> &str {
        &self.store_generation
    }

    pub fn endpoint_id(&self) -> &str {
        &self.endpoint_id
    }

    /// Highest committed journal position.
    pub fn cursor(&self) -> Result<i64, JournalError> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(MAX(ingest_seq), 0) FROM observations",
            [],
            |row| row.get(0),
        )?)
    }

    /// A consistent projection at the committed cursor, read in one short
    /// read transaction on the writer's connection.
    pub fn snapshot(&mut self) -> Result<(i64, FleetSnapshot), JournalError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let cursor: i64 = tx.query_row(
            "SELECT COALESCE(MAX(ingest_seq), 0) FROM observations",
            [],
            |row| row.get(0),
        )?;
        let snapshot = FleetSnapshot {
            view_revision: format_cursor(cursor),
            sessions: projection::sessions(&tx)?,
            attention: projection::open_attention(&tx)?,
            counts: projection::counts(&tx)?,
        };
        tx.finish()?;
        Ok((cursor, snapshot))
    }

    /// Full upserts for every entity a committed change touched.
    pub fn patch_for(
        &self,
        from_cursor: i64,
        change: &Change,
    ) -> Result<ProjectionPatch, JournalError> {
        let mut session_upserts = Vec::with_capacity(change.session_ids.len());
        for id in &change.session_ids {
            session_upserts.push(projection::session(&self.conn, id)?);
        }
        let mut attention_upserts = Vec::with_capacity(change.attention_ids.len());
        for id in &change.attention_ids {
            attention_upserts.push(projection::attention(&self.conn, id)?);
        }
        Ok(ProjectionPatch {
            from_cursor: format_cursor(from_cursor),
            to_cursor: format_cursor(change.cursor),
            view_revision: format_cursor(change.cursor),
            session_upserts,
            attention_upserts,
            tombstones: Vec::new(),
            counts: projection::counts(&self.conn)?,
        })
    }

    fn insert_observation(
        tx: &rusqlite::Transaction<'_>,
        source_id: &str,
        source_epoch: &str,
        native_event: &str,
        payload: &serde_json::Value,
        now_ms: i64,
    ) -> Result<(String, i64), JournalError> {
        let observation_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO observations (observation_id, source_id, source_epoch, native_event,
               captured_wall_ms, received_wall_ms, payload_version, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, 1, ?6)",
            params![
                observation_id,
                source_id,
                source_epoch,
                native_event,
                now_ms,
                payload.to_string()
            ],
        )?;
        Ok((observation_id, tx.last_insert_rowid()))
    }

    /// Seeds the minimal M0 fixture once per store: Session, ProcessKey,
    /// activation, SurfaceBinding, Turn and one AttentionItem, journaled as a
    /// single `FIXTURE_SEEDED` observation.
    fn ensure_fixture(&mut self, now_ms: i64) -> Result<(), JournalError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let seeded: Option<String> = tx
            .query_row(
                "SELECT value FROM store_meta WHERE key = 'fixture_seeded'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if seeded.is_some() {
            return Ok(());
        }
        let payload = serde_json::json!({ "fixture": FIXTURE_PROFILE });
        let (observation_id, cursor) = Self::insert_observation(
            &tx,
            SOURCE_FIXTURE,
            &self.source_epoch,
            "FIXTURE_SEEDED",
            &payload,
            now_ms,
        )?;

        let namespace_id = Uuid::new_v4().to_string();
        let session_id = Uuid::new_v4().to_string();
        let process_id = Uuid::new_v4().to_string();
        let execution_id = Uuid::new_v4().to_string();
        let binding_id = Uuid::new_v4().to_string();
        let turn_id = Uuid::new_v4().to_string();
        let attention_id = Uuid::new_v4().to_string();

        tx.execute(
            "INSERT INTO provider_namespaces (id, provider, endpoint_id, profile_ref) VALUES (?1, ?2, ?3, ?4)",
            params![namespace_id, FIXTURE_PROVIDER, self.endpoint_id, FIXTURE_PROFILE],
        )?;
        tx.execute(
            "INSERT INTO sessions (id, namespace_id, native_session_id, record_state, display_name, fixture, revision)
             VALUES (?1, ?2, ?3, 'KNOWN', 'Fixture worker', 1, ?4)",
            params![session_id, namespace_id, FIXTURE_NATIVE_SESSION, cursor],
        )?;
        tx.execute(
            "INSERT INTO process_incarnations (id, endpoint_id, boot_id, pid, start_seconds, start_microseconds, executable_identity)
             VALUES (?1, ?2, 'fixture-boot', 4242, 1759000000, 0, 'fixture:synthetic-process')",
            params![process_id, self.endpoint_id],
        )?;
        tx.execute(
            "INSERT INTO executions (id, session_id, activation, mode, presence)
             VALUES (?1, ?2, 1, 'terminal_embedded', 'LIVE')",
            params![execution_id, session_id],
        )?;
        tx.execute(
            "INSERT INTO execution_processes (execution_id, process_id, role) VALUES (?1, ?2, 'provider')",
            params![execution_id, process_id],
        )?;
        tx.execute(
            "INSERT INTO surface_bindings (id, session_id, execution_id, surface_kind, native_locator, proof, revision, valid)
             VALUES (?1, ?2, ?3, 'fixture', 'fixture:surface-1', 'FIXTURE', ?4, 1)",
            params![binding_id, session_id, execution_id, cursor],
        )?;
        tx.execute(
            "INSERT INTO turns (id, session_id, execution_id, native_turn_id, identity_kind, state, created_cursor)
             VALUES (?1, ?2, ?3, ?4, 'NATIVE', 'COMPLETED', ?5)",
            params![turn_id, session_id, execution_id, FIXTURE_NATIVE_TURN, cursor],
        )?;
        tx.execute(
            "INSERT INTO attention_items (id, session_id, turn_id, category, scope_kind, scope_key, priority, summary,
               created_by_observation, created_at_ms, notification_state, revision)
             VALUES (?1, ?2, ?3, 'TURN_COMPLETE', 'TURN_OUTPUT', ?3, 40, 'Fixture turn completed', ?4, ?5, 'NOT_REQUESTED', ?6)",
            params![attention_id, session_id, turn_id, observation_id, now_ms, cursor],
        )?;
        tx.execute(
            "INSERT INTO store_meta (key, value) VALUES ('fixture_seeded', ?1)",
            params![format_cursor(cursor)],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Journals an owner acknowledgement. An existing command ID with the same
    /// payload returns its recorded receipt; conflicting reuse is rejected
    /// before any new-mutation precondition (SPEC §18.3).
    pub fn acknowledge_attention(
        &mut self,
        command_id: &str,
        attention_id: &str,
        expected_revision: Option<i64>,
        now_ms: i64,
    ) -> Result<CommandOutcome, JournalError> {
        let payload = serde_json::json!({
            "action": "AcknowledgeAttention",
            "attentionId": attention_id,
            "expectedRevision": expected_revision.map(format_cursor),
        });
        let print = fingerprint(&payload);
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        let existing: Option<(String, String)> = tx
            .query_row(
                "SELECT payload_fingerprint, result_json FROM attention_commands WHERE command_id = ?1",
                params![command_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((recorded_print, result_json)) = existing {
            if recorded_print != print {
                return Err(JournalError::Conflict {
                    detail: format!(
                        "command {command_id} was already used for a different payload"
                    ),
                });
            }
            let mut receipt: CommandReceipt =
                serde_json::from_str(&result_json).map_err(|error| JournalError::Invalid {
                    detail: error.to_string(),
                })?;
            receipt.status = ReceiptStatus::AlreadyCommitted;
            return Ok(CommandOutcome {
                receipt,
                change: None,
            });
        }

        let current: Option<(String, i64)> = tx
            .query_row(
                "SELECT session_id, revision FROM attention_items WHERE id = ?1",
                params![attention_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((session_id, revision)) = current else {
            return Err(JournalError::NotFound {
                entity: "attention",
                id: attention_id.to_owned(),
            });
        };
        if let Some(expected) = expected_revision
            && expected != revision
        {
            return Err(JournalError::Conflict {
                detail: format!(
                    "attention {attention_id} is at revision {revision}, not {expected}"
                ),
            });
        }

        let (observation_id, cursor) = Self::insert_observation(
            &tx,
            SOURCE_OWNER,
            &self.source_epoch,
            "OWNER_COMMAND",
            &payload,
            now_ms,
        )?;
        tx.execute(
            "UPDATE attention_items
               SET acknowledged_at_ms = COALESCE(acknowledged_at_ms, ?2), revision = ?3
             WHERE id = ?1",
            params![attention_id, now_ms, cursor],
        )?;
        let receipt = CommandReceipt {
            command_id: command_id.to_owned(),
            status: ReceiptStatus::Committed,
            cursor: format_cursor(cursor),
            target_revision: format_cursor(cursor),
        };
        let result_json =
            serde_json::to_string(&receipt).map_err(|error| JournalError::Invalid {
                detail: error.to_string(),
            })?;
        tx.execute(
            "INSERT INTO attention_commands (command_id, attention_id, action, payload_json, payload_fingerprint,
               result_json, observation_id)
             VALUES (?1, ?2, 'AcknowledgeAttention', ?3, ?4, ?5, ?6)",
            params![command_id, attention_id, payload.to_string(), print, result_json, observation_id],
        )?;
        tx.commit()?;
        Ok(CommandOutcome {
            receipt,
            change: Some(Change {
                cursor,
                session_ids: vec![session_id],
                attention_ids: vec![attention_id.to_owned()],
            }),
        })
    }

    /// Records a notification submission outcome. OS acceptance is not proof
    /// the owner saw a banner (SPEC §7.6); the attention item is unaffected
    /// except for its delivery state.
    pub fn record_notification_state(
        &mut self,
        request_id: &str,
        state: &NotificationState,
        detail: &str,
        now_ms: i64,
    ) -> Result<Change, JournalError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let target: Option<(String, String)> = tx
            .query_row(
                "SELECT o.attention_id, a.session_id FROM notification_outbox o
                   JOIN attention_items a ON a.id = o.attention_id
                 WHERE o.request_id = ?1",
                params![request_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((attention_id, session_id)) = target else {
            return Err(JournalError::NotFound {
                entity: "notification request",
                id: request_id.to_owned(),
            });
        };
        let state_text = enum_text(state);
        let payload = serde_json::json!({
            "requestId": request_id,
            "attentionId": attention_id,
            "state": state_text,
            "detail": detail,
        });
        let (_, cursor) = Self::insert_observation(
            &tx,
            SOURCE_NOTIFICATIONS,
            &self.source_epoch,
            "NOTIFICATION_DELIVERY_RECORDED",
            &payload,
            now_ms,
        )?;
        tx.execute(
            "UPDATE notification_outbox SET state = ?2, updated_at_ms = ?3, outcome_detail = ?4 WHERE request_id = ?1",
            params![request_id, state_text, now_ms, detail],
        )?;
        tx.execute(
            "UPDATE attention_items SET notification_state = ?2, revision = ?3 WHERE id = ?1",
            params![attention_id, state_text, cursor],
        )?;
        tx.commit()?;
        Ok(Change {
            cursor,
            session_ids: vec![session_id],
            attention_ids: vec![attention_id],
        })
    }

    /// Re-reads an attention item for a native notification response. Only
    /// validated internal IDs reach here.
    pub fn attention_target(&self, attention_id: &str) -> Result<AttentionTarget, JournalError> {
        self.conn
            .query_row(
                "SELECT session_id, resolved_at_ms IS NULL FROM attention_items WHERE id = ?1",
                params![attention_id],
                |row| {
                    Ok(AttentionTarget {
                        attention_id: attention_id.to_owned(),
                        session_id: row.get(0)?,
                        outstanding: row.get(1)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| JournalError::NotFound {
                entity: "attention",
                id: attention_id.to_owned(),
            })
    }

    /// Qualification only: commit a new completed fixture turn, its owner
    /// attention item and a PENDING notification intent in one transaction.
    #[cfg(feature = "qualification")]
    pub fn raise_qualification_attention(
        &mut self,
        label: &str,
        now_ms: i64,
    ) -> Result<RaisedAttention, JournalError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (session_id, execution_id): (String, String) = tx.query_row(
            "SELECT s.id, e.id FROM sessions s JOIN executions e ON e.session_id = s.id
              WHERE s.fixture = 1 ORDER BY e.activation DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let payload = serde_json::json!({ "label": label });
        let (observation_id, cursor) = Self::insert_observation(
            &tx,
            "qualification",
            &self.source_epoch,
            "QUALIFY_ATTENTION_RAISED",
            &payload,
            now_ms,
        )?;
        let turn_id = Uuid::new_v4().to_string();
        let attention_id = Uuid::new_v4().to_string();
        let request_id = Uuid::new_v4().to_string();
        let native_turn = format!("m0a-qualification-turn-{cursor}");
        let summary = format!("Fixture turn completed — {label}");
        tx.execute(
            "INSERT INTO turns (id, session_id, execution_id, native_turn_id, identity_kind, state, created_cursor)
             VALUES (?1, ?2, ?3, ?4, 'NATIVE', 'COMPLETED', ?5)",
            params![turn_id, session_id, execution_id, native_turn, cursor],
        )?;
        tx.execute(
            "INSERT INTO attention_items (id, session_id, turn_id, category, scope_kind, scope_key, priority, summary,
               created_by_observation, created_at_ms, notification_state, revision)
             VALUES (?1, ?2, ?3, 'TURN_COMPLETE', 'TURN_OUTPUT', ?3, 40, ?4, ?5, ?6, 'PENDING', ?7)",
            params![attention_id, session_id, turn_id, summary, observation_id, now_ms, cursor],
        )?;
        tx.execute(
            "INSERT INTO notification_outbox (request_id, attention_id, state, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, 'PENDING', ?3, ?3)",
            params![request_id, attention_id, now_ms],
        )?;
        tx.execute(
            "UPDATE sessions SET revision = ?2 WHERE id = ?1",
            params![session_id, cursor],
        )?;
        tx.commit()?;
        Ok(RaisedAttention {
            change: Change {
                cursor,
                session_ids: vec![session_id.clone()],
                attention_ids: vec![attention_id.clone()],
            },
            intent: NotificationIntent {
                request_id,
                attention_id,
                session_id,
                title: "Fixture worker finished a turn".to_owned(),
                body: summary,
            },
        })
    }

    pub fn sqlite_diagnostics(&self) -> Result<SqliteDiagnostics, JournalError> {
        let (version, source_id) = verify_engine(&self.conn)?;
        let mut statement = self.conn.prepare("PRAGMA compile_options")?;
        let compile_options = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let journal_mode: String = self
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        let synchronous: u8 = self
            .conn
            .query_row("PRAGMA synchronous", [], |row| row.get(0))?;
        let foreign_keys: bool = self
            .conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
        let fullfsync: bool = self
            .conn
            .query_row("PRAGMA fullfsync", [], |row| row.get(0))?;
        Ok(SqliteDiagnostics {
            version,
            source_id,
            compile_options,
            journal_mode,
            synchronous,
            foreign_keys,
            fullfsync,
            database_path: self.path.display().to_string(),
            cursor: format_cursor(self.cursor()?),
            schema_version: SCHEMA_VERSION,
        })
    }
}

fn meta_get_or_insert(tx: &rusqlite::Transaction<'_>, key: &str) -> Result<String, JournalError> {
    let existing: Option<String> = tx
        .query_row(
            "SELECT value FROM store_meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(value) = existing {
        return Ok(value);
    }
    let value = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO store_meta (key, value) VALUES (?1, ?2)",
        params![key, value],
    )?;
    Ok(value)
}
