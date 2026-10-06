//! Versioned migrations. The M0 schema holds only the records the M0 gates
//! need (MILESTONES "Prototype contract"); M1 completes the canonical schema.

use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

use crate::JournalError;

pub const SCHEMA_VERSION: u32 = 2;

const MIGRATION_0001: &str = r"
CREATE TABLE store_meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;

-- The journal: every accepted observation and owner command. ingest_seq is
-- the local commit cursor; it is monotonic, not gap-free (SPEC 5.4).
CREATE TABLE observations (
  ingest_seq        INTEGER PRIMARY KEY AUTOINCREMENT,
  observation_id    TEXT NOT NULL UNIQUE,
  source_id         TEXT NOT NULL,
  source_epoch      TEXT NOT NULL,
  source_sequence   TEXT,
  native_event      TEXT NOT NULL,
  captured_wall_ms  INTEGER NOT NULL,
  received_wall_ms  INTEGER NOT NULL,
  payload_version   INTEGER NOT NULL,
  payload_json      TEXT NOT NULL
) STRICT;

CREATE TABLE provider_namespaces (
  id           TEXT PRIMARY KEY,
  provider     TEXT NOT NULL,
  endpoint_id  TEXT NOT NULL,
  profile_ref  TEXT NOT NULL,
  UNIQUE(provider, endpoint_id, profile_ref)
) STRICT;

CREATE TABLE sessions (
  id                 TEXT PRIMARY KEY,
  namespace_id       TEXT NOT NULL REFERENCES provider_namespaces(id),
  native_session_id  TEXT NOT NULL,
  record_state       TEXT NOT NULL,
  display_name       TEXT NOT NULL,
  fixture            INTEGER NOT NULL,
  revision           INTEGER NOT NULL,
  UNIQUE(namespace_id, native_session_id)
) STRICT;

-- ProcessKey: endpoint, boot, PID and kernel birth. PID alone is never identity.
CREATE TABLE process_incarnations (
  id                   TEXT PRIMARY KEY,
  endpoint_id          TEXT NOT NULL,
  boot_id              TEXT NOT NULL,
  pid                  INTEGER NOT NULL,
  start_seconds        INTEGER NOT NULL,
  start_microseconds   INTEGER NOT NULL,
  executable_identity  TEXT NOT NULL,
  UNIQUE(endpoint_id, boot_id, pid, start_seconds, start_microseconds)
) STRICT;

-- One logical activation of a session in a runtime.
CREATE TABLE executions (
  id          TEXT PRIMARY KEY,
  session_id  TEXT NOT NULL REFERENCES sessions(id),
  activation  INTEGER NOT NULL,
  mode        TEXT NOT NULL,
  presence    TEXT NOT NULL,
  UNIQUE(session_id, activation)
) STRICT;

CREATE TABLE execution_processes (
  execution_id  TEXT NOT NULL REFERENCES executions(id),
  process_id    TEXT NOT NULL REFERENCES process_incarnations(id),
  role          TEXT NOT NULL,
  PRIMARY KEY (execution_id, process_id)
) STRICT;

CREATE TABLE surface_bindings (
  id              TEXT PRIMARY KEY,
  session_id      TEXT NOT NULL REFERENCES sessions(id),
  execution_id    TEXT NOT NULL REFERENCES executions(id),
  surface_kind    TEXT NOT NULL,
  native_locator  TEXT NOT NULL,
  proof           TEXT NOT NULL,
  revision        INTEGER NOT NULL,
  valid           INTEGER NOT NULL
) STRICT;

CREATE TABLE turns (
  id              TEXT PRIMARY KEY,
  session_id      TEXT NOT NULL REFERENCES sessions(id),
  execution_id    TEXT REFERENCES executions(id),
  native_turn_id  TEXT,
  identity_kind   TEXT NOT NULL,
  state           TEXT NOT NULL,
  created_cursor  INTEGER NOT NULL,
  UNIQUE(session_id, native_turn_id)
) STRICT;

CREATE TABLE attention_items (
  id                      TEXT PRIMARY KEY,
  session_id              TEXT NOT NULL REFERENCES sessions(id),
  turn_id                 TEXT REFERENCES turns(id),
  category                TEXT NOT NULL,
  scope_kind              TEXT NOT NULL,
  scope_key               TEXT NOT NULL,
  priority                INTEGER NOT NULL,
  summary                 TEXT,
  created_by_observation  TEXT NOT NULL REFERENCES observations(observation_id),
  created_at_ms           INTEGER NOT NULL,
  acknowledged_at_ms      INTEGER,
  resolved_at_ms          INTEGER,
  notification_state      TEXT NOT NULL,
  revision                INTEGER NOT NULL,
  UNIQUE(session_id, scope_kind, scope_key)
) STRICT;

-- Owner commands are journal entries with stable IDs (SPEC 5.5, 18.3).
CREATE TABLE attention_commands (
  command_id           TEXT PRIMARY KEY,
  attention_id         TEXT NOT NULL REFERENCES attention_items(id),
  action               TEXT NOT NULL,
  payload_json         TEXT NOT NULL,
  payload_fingerprint  TEXT NOT NULL,
  result_json          TEXT NOT NULL,
  observation_id       TEXT NOT NULL UNIQUE REFERENCES observations(observation_id)
) STRICT;

-- Notification intent is created in the same transaction as its attention.
CREATE TABLE notification_outbox (
  request_id       TEXT PRIMARY KEY,
  attention_id     TEXT NOT NULL REFERENCES attention_items(id),
  state            TEXT NOT NULL,
  created_at_ms    INTEGER NOT NULL,
  updated_at_ms    INTEGER NOT NULL,
  outcome_detail   TEXT
) STRICT;
";

/// M0B: the proof records a direct-Claude identity join and an exact Terminal
/// route need (SPEC §4.4, §4.6, §13.3). A binding keeps the ProcessKey,
/// executable, controlling device and Terminal incarnation it was proven
/// with; the TTY path is only its locator. Activations end by ID with a
/// reason, and every Return attempt is journaled with its evidence.
const MIGRATION_0002: &str = r"
ALTER TABLE sessions ADD COLUMN provider_kind TEXT;
ALTER TABLE sessions ADD COLUMN provider_status TEXT;
ALTER TABLE sessions ADD COLUMN provider_waiting_for TEXT;
-- 1 while the session's row is in the latest applied inventory.
ALTER TABLE sessions ADD COLUMN inventory_present INTEGER NOT NULL DEFAULT 0;

ALTER TABLE executions ADD COLUMN process_id TEXT REFERENCES process_incarnations(id);
ALTER TABLE executions ADD COLUMN device_number INTEGER;
ALTER TABLE executions ADD COLUMN started_cursor INTEGER;
ALTER TABLE executions ADD COLUMN ended_cursor INTEGER;
ALTER TABLE executions ADD COLUMN end_reason TEXT;
-- Why the activation has no valid binding (e.g. NO_MATCHING_TAB).
ALTER TABLE executions ADD COLUMN surface_status TEXT;

ALTER TABLE surface_bindings ADD COLUMN process_id TEXT REFERENCES process_incarnations(id);
ALTER TABLE surface_bindings ADD COLUMN executable_identity TEXT;
ALTER TABLE surface_bindings ADD COLUMN device_number INTEGER;
ALTER TABLE surface_bindings ADD COLUMN terminal_generation TEXT;
ALTER TABLE surface_bindings ADD COLUMN window_hint INTEGER;
ALTER TABLE surface_bindings ADD COLUMN tab_hint INTEGER;
ALTER TABLE surface_bindings ADD COLUMN evidence_observation TEXT REFERENCES observations(observation_id);
ALTER TABLE surface_bindings ADD COLUMN proof_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE surface_bindings ADD COLUMN invalidated_reason TEXT;
ALTER TABLE surface_bindings ADD COLUMN invalidated_cursor INTEGER;

CREATE INDEX executions_live ON executions(presence, session_id);
CREATE INDEX surface_bindings_execution ON surface_bindings(execution_id, valid);

CREATE TABLE route_results (
  request_id            TEXT PRIMARY KEY,
  session_id            TEXT NOT NULL REFERENCES sessions(id),
  binding_id            TEXT REFERENCES surface_bindings(id),
  binding_revision      INTEGER,
  surface_result        TEXT NOT NULL,
  session_verification  TEXT NOT NULL,
  input_readiness       TEXT NOT NULL,
  reason_code           TEXT NOT NULL,
  focus_performed       INTEGER NOT NULL,
  latency_ms            INTEGER NOT NULL,
  started_at_ms         INTEGER NOT NULL,
  recorded_at_ms        INTEGER NOT NULL,
  observation_id        TEXT NOT NULL UNIQUE REFERENCES observations(observation_id),
  evidence_json         TEXT NOT NULL
) STRICT;
CREATE INDEX route_results_session ON route_results(session_id, recorded_at_ms);
";

struct Migration {
    id: u32,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        id: 1,
        name: "m0-fixture-schema",
        sql: MIGRATION_0001,
    },
    Migration {
        id: 2,
        name: "m0b-identity-and-routes",
        sql: MIGRATION_0002,
    },
];

fn checksum(sql: &str) -> String {
    let digest = Sha256::digest(sql.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Applies pending migrations inside one immediate transaction each, refusing
/// a store written by a newer schema (SPEC §9.4).
pub fn migrate(conn: &mut Connection, now_ms: i64) -> Result<u32, JournalError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
           id            INTEGER PRIMARY KEY,
           name          TEXT NOT NULL,
           checksum      TEXT NOT NULL,
           applied_at_ms INTEGER NOT NULL
         ) STRICT;",
    )?;
    let applied: u32 = conn.query_row(
        "SELECT COALESCE(MAX(id), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;
    if applied > SCHEMA_VERSION {
        return Err(JournalError::SchemaTooNew { found: applied });
    }
    for migration in MIGRATIONS {
        let sum = checksum(migration.sql);
        if migration.id <= applied {
            let recorded: String = conn.query_row(
                "SELECT checksum FROM schema_migrations WHERE id = ?1",
                params![migration.id],
                |row| row.get(0),
            )?;
            if recorded != sum {
                return Err(JournalError::MigrationChecksum { id: migration.id });
            }
            continue;
        }
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch(migration.sql)?;
        tx.execute(
            "INSERT INTO schema_migrations (id, name, checksum, applied_at_ms) VALUES (?1, ?2, ?3, ?4)",
            params![migration.id, migration.name, sum, now_ms],
        )?;
        tx.commit()?;
    }
    Ok(SCHEMA_VERSION)
}

/// Checks, without changing anything, that `conn` holds exactly this
/// binary's migrations 1..=SCHEMA_VERSION with matching checksums.
pub(crate) fn verify_applied(conn: &Connection) -> Result<u32, String> {
    let mut statement = conn
        .prepare("SELECT id, checksum FROM schema_migrations ORDER BY id")
        .map_err(|error| format!("schema_migrations unreadable: {error}"))?;
    let applied = statement
        .query_map([], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
        })
        .and_then(Iterator::collect::<Result<Vec<_>, _>>)
        .map_err(|error| format!("schema_migrations unreadable: {error}"))?;
    let found = applied.last().map_or(0, |(id, _)| *id);
    if found != SCHEMA_VERSION || applied.len() != MIGRATIONS.len() {
        return Err(format!(
            "schema version {found} ({} migrations) is not the supported {SCHEMA_VERSION}",
            applied.len()
        ));
    }
    for ((id, recorded), migration) in applied.iter().zip(MIGRATIONS) {
        if *id != migration.id || *recorded != checksum(migration.sql) {
            return Err(format!("migration {id} differs from this binary's"));
        }
    }
    Ok(found)
}
