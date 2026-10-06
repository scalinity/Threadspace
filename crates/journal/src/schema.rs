//! Versioned migrations. The M0 schema holds only the records the M0 gates
//! need (MILESTONES "Prototype contract"); M1 completes the canonical schema.

use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

use crate::JournalError;

pub const SCHEMA_VERSION: u32 = 1;

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

struct Migration {
    id: u32,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[Migration {
    id: 1,
    name: "m0-fixture-schema",
    sql: MIGRATION_0001,
}];

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
