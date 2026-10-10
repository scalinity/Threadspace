//! The companion's single-writer SQLite journal (SPEC §9).
//!
//! One companion process owns writes under `WriterLock`. The connection runs
//! WAL, `synchronous=FULL` and `foreign_keys=ON`, and refuses to open unless
//! the linked engine is exactly SQLite 3.53.4 (docs/decisions/D-0001). A
//! receipt is produced only after `COMMIT` returns.

mod admission;
mod backup;
mod baseline;
mod canonical;
#[cfg(feature = "qualification")]
mod crash;
mod identity;
mod lock;
#[cfg(feature = "qualification")]
pub mod latency;
mod materialize;
mod owner;
pub mod paging;
mod projection;
#[cfg(feature = "qualification")]
mod qualification;
mod schema;

use std::path::{Path, PathBuf};

use rusqlite::config::DbConfig;
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use threadspace_contracts::canonical::command::{OwnerAction, OwnerCommand};
use threadspace_contracts::canonical::fact::{
    BindingMethod, BindingProof, CanonicalRefs, Delivery, EvidenceClass, ExecutionMode,
    AttachedPresence, FactPayload, NativeFactDraft, NativeRefs, ResolvedFact, TurnOutcome,
};
use threadspace_contracts::canonical::keys::{NativeExecutionRef, NativeSessionRef, NativeSurfaceRef};
use threadspace_contracts::canonical::FACT_PAYLOAD_VERSION;
use threadspace_contracts::cursor::format_cursor;
use threadspace_contracts::route::ProcessKey;
use threadspace_state_engine::engine::Engine;
use threadspace_state_engine::ids::{Allocator, RandomAllocator};
use threadspace_state_engine::resolve::IdentityIndex;
use threadspace_contracts::diagnostics::SqliteDiagnostics;
use threadspace_contracts::projection::{FleetSnapshot, NotificationState, ProjectionPatch};
use threadspace_contracts::ui::CommandReceipt;
use uuid::Uuid;

pub use admission::{AdmissionReceipt, ObservationAdmission};
#[cfg(feature = "qualification")]
pub use backup::file_sha256;
pub use backup::{
    BackupError, BackupInfo, RestoreOutcome, WalCheckpoint, backup_store_into, restore_backup,
    verify_backup,
};
#[cfg(feature = "qualification")]
pub use crash::{CRASH_AT_ENV, CRASH_POINT_ENV, CrashPlan, CrashPoint};
#[cfg(feature = "qualification")]
pub use identity::ObservationExport;
pub use identity::{
    ActivationChange, ApplyOutcome, BindingRow, DiscoveryApplication, LiveExecutionRow,
    ObservedSessionRecord, ProcessRecord, RouteTargetRow, SessionWait, SurfaceRecord,
};
pub use canonical::{
    BatchOutcome, CHECKPOINT_INTERVAL, EnvelopeAdmission, OBSERVATION_MAX_BYTES, RecordOutcome,
    ReplayDigest, validate_envelope,
};
pub use lock::{LockError, WriterLock};
pub use materialize::TableDifference;
pub use schema::{SCHEMA_VERSION, catalog as migration_catalog};

pub const REQUIRED_SQLITE_VERSION: &str = "3.53.4";
pub const REQUIRED_SQLITE_SOURCE_ID: &str =
    "2026-07-24 19:02:57 bf7c7f30031888f4e796e429ab3978879485813aaca6f641c7b33e4e09459bcc";

const FIXTURE_PROVIDER: &str = threadspace_state_engine::FIXTURE_PROVIDER;
const FIXTURE_PROFILE: &str = threadspace_state_engine::FIXTURE_PROFILE;
const FIXTURE_NATIVE_SESSION: &str = "m0a-fixture-session-1";
const FIXTURE_NATIVE_TURN: &str = "m0a-fixture-turn-1";
const SOURCE_FIXTURE: &str = "m0a.fixture";
const SOURCE_OWNER: &str = "owner";
const SOURCE_NOTIFICATIONS: &str = "companion.notifications";
const FIXTURE_ACTIVATION: &str = "fixture-activation-1";

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

impl JournalError {
    /// The store refused this input itself (a constraint or an invalid
    /// record), so retrying it unchanged fails the same way.
    pub fn is_refusal(&self) -> bool {
        match self {
            Self::Invalid { .. } => true,
            Self::Sqlite(rusqlite::Error::SqliteFailure(error, _)) => {
                error.code == rusqlite::ErrorCode::ConstraintViolation
            }
            _ => false,
        }
    }
}

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
    /// The canonical reducer's state: equal to the journal's replay, and to
    /// the materialized projections, after every commit.
    engine: Engine,
    /// Recorded native key → canonical ID assignments.
    index: IdentityIndex,
    allocator: Box<dyn Allocator + Send>,
    entries_since_checkpoint: u64,
    /// Set if the engine could not be rebuilt after a failed transaction.
    poisoned: bool,
    #[cfg(feature = "qualification")]
    crash: crash::CrashState,
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

/// Refuses an existing store this binary cannot run (SPEC §9.4) before the
/// writable open, whose WAL pragma, migrations and bootstrap would otherwise
/// change it first. The connection that reads the versions depends on the
/// sidecars present, so that it writes nothing and creates no file in the
/// store directory:
/// - a `-wal` may hold the only copy of committed rows, so it is always
///   read, and never with `immutable`, which ignores it. Beside a `-shm`, it
///   is read with `readonly_shm`, which indexes it in private memory instead
///   of rebuilding the `-shm`. With no `-shm`, SQLite must create one to read
///   the WAL at all (`readonly_shm` then fails to open, and an exclusive
///   lock fails on a read-only descriptor), so the main file and `-wal` are
///   read from an `InspectionCopy` in a private temporary directory, where
///   that `-shm` lands instead;
/// - a `-journal` may be hot (a crash in rollback mode, as while a restored
///   backup converts to WAL). SQLite rolls it back before any reader sees
///   committed rows, and a read-only connection would fail on every start,
///   so this one case opens writable: the rollback restores the last
///   committed bytes and is the only write the preflight can make;
/// - with neither, the main file is the whole store, and `immutable` keeps a
///   WAL-mode header from creating a `-wal` and `-shm`.
fn preflight(path: &Path) -> Result<(), JournalError> {
    if !std::fs::metadata(path).is_ok_and(|meta| meta.len() > 0) {
        return Ok(());
    }
    let sidecar = |suffix: &str| {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        Path::new(&name).exists()
    };
    let mut target = path.to_path_buf();
    // Declared before the connection, so that it is removed only after the
    // connection has closed, on every path out of this function.
    let copy;
    let (access, query) = if sidecar("-wal") {
        if sidecar("-shm") {
            (OpenFlags::SQLITE_OPEN_READ_ONLY, "?readonly_shm=1")
        } else {
            copy = InspectionCopy::new(path, &std::env::temp_dir())?;
            target = copy.main();
            (OpenFlags::SQLITE_OPEN_READ_ONLY, "")
        }
    } else if sidecar("-journal") {
        (OpenFlags::SQLITE_OPEN_READ_WRITE, "")
    } else {
        (OpenFlags::SQLITE_OPEN_READ_ONLY, "?immutable=1")
    };
    let mut uri = String::from("file:");
    for ch in target.to_string_lossy().chars() {
        match ch {
            '%' | '?' | '#' => uri.push_str(&format!("%{:02X}", u32::from(ch))),
            _ => uri.push(ch),
        }
    }
    uri.push_str(query);
    let conn = Connection::open_with_flags(
        uri,
        access | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    // Closing must never copy the WAL into the main file.
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)?;
    verify_engine(&conn)?;
    schema::refuse_newer(&conn)
}

/// A store's main file and `-wal`, copied into a new directory of mode 0700
/// under `parent`, so that a preflight read of a WAL with no `-shm` creates
/// its `-shm` there rather than beside the store. The directory and
/// everything SQLite created in it are removed when this is dropped.
struct InspectionCopy(PathBuf);

impl InspectionCopy {
    /// The caller of `Journal::open` holds the store's `WriterLock`, so no
    /// writer changes the main file or `-wal` while they are copied, and the
    /// two copies are one committed state. Where the temporary directory
    /// shares the store's APFS volume, `std::fs::copy` clones each file.
    fn new(main: &Path, parent: &Path) -> Result<Self, JournalError> {
        use std::os::unix::fs::DirBuilderExt;
        let failed = |step: &str, error: std::io::Error| JournalError::Invalid {
            detail: format!("preflight inspection copy: {step}: {error}"),
        };
        let mut wal = main.as_os_str().to_owned();
        wal.push("-wal");
        let dir = parent.join(format!("threadspace-preflight-{}", Uuid::new_v4()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|error| failed("create directory", error))?;
        let copy = Self(dir);
        std::fs::copy(main, copy.main()).map_err(|error| failed("copy main file", error))?;
        std::fs::copy(&wal, copy.0.join("journal.sqlite3-wal"))
            .map_err(|error| failed("copy -wal", error))?;
        Ok(copy)
    }

    fn main(&self) -> PathBuf {
        self.0.join("journal.sqlite3")
    }
}

impl Drop for InspectionCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn enum_text<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => text,
        _ => String::new(),
    }
}

impl Journal {
    /// Opens (creating if needed) the journal at `path`. The caller must hold
    /// the `WriterLock` for the containing store directory.
    pub fn open(path: &Path, source_epoch: &str, now_ms: i64) -> Result<Self, JournalError> {
        Self::open_with(path, source_epoch, now_ms, Box::new(RandomAllocator), true)
    }

    /// Opens with an explicit identity allocator (seeded in reproducible
    /// synthetic runs; random in the companion). `seed_fixture` is false
    /// only for bare synthetic stores that must hold nothing but their
    /// scenario.
    pub fn open_with(
        path: &Path,
        source_epoch: &str,
        now_ms: i64,
        mut allocator: Box<dyn Allocator + Send>,
        seed_fixture: bool,
    ) -> Result<Self, JournalError> {
        #[cfg(feature = "qualification")]
        let crash = crash::CrashState::from_env()?;
        preflight(path)?;
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
        let store_generation = meta_get_or_insert(&tx, "store_generation", allocator.as_mut())?;
        let endpoint_id = meta_get_or_insert(&tx, "endpoint_id", allocator.as_mut())?;
        tx.commit()?;

        Self::bootstrap_canonical(&mut conn, now_ms)?;
        let (engine, replayed, upgraded) = canonical::load_engine(&conn, &endpoint_id)?;
        if upgraded {
            canonical::persist_upgrade(&mut conn, &engine.state, now_ms)?;
        }
        let index = canonical::load_index(&conn)?;

        let mut journal = Self {
            conn,
            path: path.to_path_buf(),
            store_generation,
            endpoint_id,
            source_epoch: source_epoch.to_owned(),
            engine,
            index,
            allocator,
            entries_since_checkpoint: replayed,
            poisoned: false,
            #[cfg(feature = "qualification")]
            crash,
        };
        if seed_fixture {
            journal.ensure_fixture(now_ms)?;
        }
        Ok(journal)
    }

    /// Gives a store its first checkpoint: the baseline of an M0 store's
    /// rows, or an empty state at the current cursor.
    fn bootstrap_canonical(conn: &mut Connection, now_ms: i64) -> Result<(), JournalError> {
        let checkpoints: i64 =
            conn.query_row("SELECT COUNT(*) FROM projection_checkpoints", [], |row| row.get(0))?;
        if checkpoints > 0 {
            return Ok(());
        }
        let (state, assignments, origin) = if baseline::needed(conn)? {
            let (state, assignments) = baseline::from_m0(conn)?;
            (state, assignments, "M0_BASELINE")
        } else {
            let mut engine = Engine::empty();
            engine.state.through_cursor =
                conn.query_row("SELECT COALESCE(MAX(ingest_seq), 0) FROM observations", [], |row| row.get(0))?;
            (engine.state, Vec::new(), "EMPTY")
        };
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for assignment in &assignments {
            tx.execute(
                "INSERT INTO identity_assignments (native_key, entity, canonical_id, ingest_seq)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    assignment.native_key,
                    assignment.entity.as_str(),
                    assignment.id,
                    state.through_cursor
                ],
            )?;
        }
        canonical::initial_checkpoint(&tx, &state, origin, now_ms)?;
        tx.commit()?;
        Ok(())
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
        let snapshot = paging::initial_view(&tx, format_cursor(cursor), &self.engine.state.sessions)?;
        tx.finish()?;
        Ok((cursor, snapshot))
    }

    /// One page of sessions or open attention after `after`, read in a short
    /// read transaction; `view_revision` is the cursor it was read at.
    pub fn session_page(
        &mut self,
        after: Option<&str>,
        limit: u32,
    ) -> Result<
        (
            i64,
            paging::Page<threadspace_contracts::projection::SessionView>,
        ),
        JournalError,
    > {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let cursor: i64 = tx.query_row(
            "SELECT COALESCE(MAX(ingest_seq), 0) FROM observations",
            [],
            |row| row.get(0),
        )?;
        let page = paging::session_page(&tx, after, limit, paging::PAGE_BUDGET, &self.engine.state.sessions)?;
        tx.finish()?;
        Ok((cursor, page))
    }

    pub fn attention_page(
        &mut self,
        after: Option<&str>,
        limit: u32,
    ) -> Result<
        (
            i64,
            paging::Page<threadspace_contracts::projection::AttentionView>,
        ),
        JournalError,
    > {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let cursor: i64 = tx.query_row(
            "SELECT COALESCE(MAX(ingest_seq), 0) FROM observations",
            [],
            |row| row.get(0),
        )?;
        let page = paging::attention_page(&tx, after, limit, paging::PAGE_BUDGET)?;
        tx.finish()?;
        Ok((cursor, page))
    }

    /// Full upserts for every entity a committed change touched.
    pub fn patch_for(
        &self,
        from_cursor: i64,
        change: &Change,
    ) -> Result<ProjectionPatch, JournalError> {
        let mut session_upserts = Vec::with_capacity(change.session_ids.len());
        for id in &change.session_ids {
            session_upserts.push(projection::session(&self.conn, id, &self.engine.state.sessions)?);
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
            page_invalidations: Vec::new(),
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

    /// A fact the companion builds with canonical references it already
    /// holds (owner, notification and route records).
    pub(crate) fn prebuilt_fact(
        &mut self,
        observation_id: &str,
        refs: CanonicalRefs,
        provenance: EvidenceClass,
        payload: FactPayload,
    ) -> ResolvedFact {
        ResolvedFact {
            fact_id: self.allocate_id(),
            observation_id: observation_id.to_owned(),
            fact_index: 0,
            native: NativeRefs {
                attention: refs.attention_id.clone(),
                ..NativeRefs::default()
            },
            refs,
            provenance,
            causal: None,
            payload_version: FACT_PAYLOAD_VERSION,
            payload,
        }
    }

    /// Seeds the minimal M0 fixture once per store through canonical
    /// admission (bootstrap delivery: no notification intent): Session,
    /// ProcessKey, activation, SurfaceBinding, completed Turn and its
    /// AttentionItem, journaled as one `FIXTURE_SEEDED` observation.
    fn ensure_fixture(&mut self, now_ms: i64) -> Result<(), JournalError> {
        let seeded: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM store_meta WHERE key = 'fixture_seeded'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if seeded.is_some() {
            return Ok(());
        }
        let session = NativeSessionRef {
            provider: FIXTURE_PROVIDER.into(),
            profile_ref: FIXTURE_PROFILE.into(),
            native_session_id: FIXTURE_NATIVE_SESSION.into(),
        };
        let process = ProcessKey {
            endpoint_id: String::new(),
            boot_id: "fixture-boot".into(),
            pid: 4242,
            start_seconds: "1759000000".into(),
            start_microseconds: 0,
        };
        let execution = NativeExecutionRef::Activation {
            activation_ref: FIXTURE_ACTIVATION.into(),
        };
        let at = |refs: NativeRefs, payload: FactPayload| NativeFactDraft {
            refs,
            provenance: EvidenceClass::Derived,
            causal: None,
            payload,
        };
        let session_refs = NativeRefs {
            session: Some(session.clone()),
            ..NativeRefs::default()
        };
        let drafts = vec![
            at(session_refs.clone(), FactPayload::SessionIdentified {
                display_name: Some("Fixture worker".into()),
                start_source: None,
            }),
            at(
                NativeRefs { process: Some(process.clone()), ..NativeRefs::default() },
                FactPayload::ProcessObserved { executable_identity: "fixture:synthetic-process".into() },
            ),
            at(
                NativeRefs {
                    execution: Some(execution.clone()),
                    process: Some(process),
                    ..session_refs.clone()
                },
                FactPayload::ExecutionAttached {
                    mode: ExecutionMode::TerminalEmbedded,
                    presence: AttachedPresence::Live,
                    native_runtime_id: None,
                    controlling_device: None,
                },
            ),
            at(
                NativeRefs {
                    execution: Some(execution),
                    surface: Some(NativeSurfaceRef {
                        surface_kind: "fixture".into(),
                        app_generation: "fixture".into(),
                        locator: "fixture:surface-1".into(),
                        device_number: None,
                        surface_generation: "fixture".into(),
                    }),
                    ..session_refs.clone()
                },
                FactPayload::SurfaceBindingRecorded {
                    proof: BindingProof {
                        method: BindingMethod::Fixture,
                        executable_identity: Some("fixture:synthetic-process".into()),
                        window_hint: None,
                        tab_hint: None,
                        evidence: serde_json::Value::Null,
                    },
                },
            ),
            at(
                NativeRefs { turn: Some(FIXTURE_NATIVE_TURN.into()), ..session_refs },
                FactPayload::TurnOutcomeObserved {
                    outcome: TurnOutcome::Completed,
                    reason: None,
                    summary: Some("Fixture turn completed".into()),
                },
            ),
        ];
        let observation_id = self.allocate_id();
        let payload = serde_json::json!({ "fixture": FIXTURE_PROFILE });
        self.admit_internal(
            observation_id,
            SOURCE_FIXTURE,
            "FIXTURE_SEEDED",
            &payload,
            &drafts,
            Vec::new(),
            Delivery::Bootstrap,
            now_ms,
            now_ms,
            |tx, _, cursor, _| {
                tx.execute(
                    "INSERT INTO store_meta (key, value) VALUES ('fixture_seeded', ?1)",
                    params![format_cursor(cursor)],
                )?;
                Ok(())
            },
        )?;
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
        self.admit_owner_command(
            &OwnerCommand {
                command_id: command_id.to_owned(),
                attention_id: attention_id.to_owned(),
                expected_revision: expected_revision.map(format_cursor),
                action: OwnerAction::Acknowledge,
            },
            now_ms,
        )
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
        let Some(attention_id) = self
            .engine
            .state
            .outbox
            .get(request_id)
            .map(|o| o.attention_id.clone())
        else {
            return Err(JournalError::NotFound {
                entity: "notification request",
                id: request_id.to_owned(),
            });
        };
        let session_id = self
            .engine
            .state
            .attention
            .get(&attention_id)
            .map(|a| a.session_id.clone())
            .unwrap_or_default();
        let payload = serde_json::json!({
            "requestId": request_id,
            "attentionId": attention_id,
            "state": enum_text(state),
            "detail": detail,
        });
        let observation_id = self.allocate_id();
        let fact = self.prebuilt_fact(
            &observation_id,
            CanonicalRefs {
                session_id: Some(session_id.clone()),
                attention_id: Some(attention_id.clone()),
                ..CanonicalRefs::default()
            },
            EvidenceClass::Derived,
            FactPayload::NotificationDeliveryRecorded {
                request_id: request_id.to_owned(),
                state: state.clone(),
                detail: detail.chars().take(240).collect(),
            },
        );
        let (cursor, _) = self.admit_internal(
            observation_id,
            SOURCE_NOTIFICATIONS,
            "NOTIFICATION_DELIVERY_RECORDED",
            &payload,
            &[],
            vec![fact],
            Delivery::Live,
            now_ms,
            now_ms,
            |_, _, _, _| Ok(()),
        )?;
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
        self.raise_attention_on(label, None, now_ms)
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

fn meta_get_or_insert(
    tx: &rusqlite::Transaction<'_>,
    key: &str,
    allocator: &mut dyn Allocator,
) -> Result<String, JournalError> {
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
    let value = allocator.allocate();
    tx.execute(
        "INSERT INTO store_meta (key, value) VALUES (?1, ?2)",
        params![key, value],
    )?;
    Ok(value)
}
