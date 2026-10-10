//! Canonical admission (SPEC §5.1, §5.5, §8.4). One IMMEDIATE transaction per
//! batch, on the single writer:
//!
//! 1. validate the envelope and its stable observation UUID (a duplicate
//!    with identical content is ALREADY_COMMITTED and writes nothing);
//! 2. journal the observation with the adapter's retained payload only;
//! 3. resolve native keys to canonical IDs, recording new assignments;
//! 4. journal the resolved facts;
//! 5. reduce them (pure) and materialize every changed projection row,
//!    attention item, outbox intent and the applied cursor;
//! 6. commit. Receipts and notification intents exist only after commit.
//!
//! If anything fails before commit the transaction rolls back and the
//! in-memory engine is rebuilt from the store, so memory never holds state
//! the journal does not.

use rusqlite::types::Value as Sql;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::Value;
use threadspace_contracts::canonical::capture::RecordStatus;
use threadspace_contracts::canonical::JOURNAL_PAYLOAD_VERSION;
use threadspace_contracts::canonical::command::{OwnerAction, OwnerCommand};
use threadspace_contracts::canonical::envelope::{
    OBSERVATION_SCHEMA_VERSION, ObservationEnvelope, SequenceMeaning,
};
use threadspace_contracts::canonical::fact::{
    Delivery, JournalEntry, NativeFactDraft, ResolvedFact,
};
use threadspace_contracts::canonical::records::{CanonicalState, OutboxRecord, OutboxState, WaitOwnerDecision};
use threadspace_contracts::cursor::{format_cursor, parse_cursor};
use threadspace_contracts::projection::AttentionCategory;
use threadspace_contracts::ui::{CommandReceipt, ReceiptStatus};
use threadspace_state_engine::engine::{Engine, ReduceOutput};
use threadspace_state_engine::hash::{JournalDigest, sha256_hex, state_hash};
use threadspace_state_engine::normalize::Normalized;
use threadspace_state_engine::resolve::{Assignment, IdentityIndex, Resolver};
use threadspace_state_engine::{REDUCER_VERSION, command};
use uuid::Uuid;

#[cfg(feature = "qualification")]
use crate::crash::{self, CrashPoint};
use crate::materialize;
use crate::{Change, CommandOutcome, Journal, JournalError, NotificationIntent};

/// A normalized observation is at most 16 KiB (SPEC §8.5).
pub const OBSERVATION_MAX_BYTES: usize = 16 * 1024;
/// Canonical entries between checkpoints.
pub const CHECKPOINT_INTERVAL: u64 = 512;
/// Checkpoints kept (SPEC §9.3: at least two verified).
const CHECKPOINTS_KEPT: i64 = 3;
/// Admission diagnostics kept.
const DIAGNOSTICS_KEPT: i64 = 10_000;

/// One captured envelope and its adapter's pure normalization.
#[derive(Debug, Clone)]
pub struct EnvelopeAdmission<'a> {
    pub envelope: &'a ObservationEnvelope,
    pub normalized: Normalized,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordOutcome {
    pub observation_id: String,
    pub status: RecordStatus,
    pub cursor: Option<i64>,
    pub reason: Option<String>,
}

/// The committed result of one admission batch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchOutcome {
    pub records: Vec<RecordOutcome>,
    /// Entities to stream to views; `None` when nothing committed.
    pub change: Option<Change>,
    /// Live notification intents created by this commit: post-commit effects.
    pub notifications: Vec<NotificationIntent>,
    pub unresolved: u64,
    pub unsupported: u64,
    /// Actual COMMIT-call bracket. Not persisted, not canonical, and absent
    /// from release builds. Missing clock data never means zero latency.
    #[cfg(feature = "qualification")]
    pub commit_timing: Option<crate::latency::CommitTiming>,
}

/// An observation row admission writes.
pub(crate) struct ObservationRow {
    pub observation_id: String,
    pub source_id: String,
    pub source_epoch: String,
    pub source_sequence: Option<String>,
    pub sequence_meaning: Option<SequenceMeaning>,
    pub native_event: String,
    pub captured_wall_ms: i64,
    pub payload_json: String,
}

fn enum_text<T: serde::Serialize>(value: &T) -> Option<String> {
    match serde_json::to_value(value) {
        Ok(Value::String(text)) => Some(text),
        _ => None,
    }
}

fn bounded(value: &str, max: usize) -> bool {
    !value.is_empty() && value.chars().count() <= max && !value.chars().any(char::is_control)
}

/// Envelope-level checks; a failure is NOT_ACCEPTED and writes nothing.
pub fn validate_envelope(envelope: &ObservationEnvelope) -> Result<String, &'static str> {
    if envelope.schema_version != OBSERVATION_SCHEMA_VERSION {
        return Err("UNSUPPORTED_SCHEMA_VERSION");
    }
    // Only the canonical lowercase hyphenated form: receipts name the ID as
    // given, and callers match them by it.
    let id = Uuid::parse_str(&envelope.observation_id)
        .map_err(|_| "INVALID_OBSERVATION_ID")?
        .hyphenated()
        .to_string();
    if id != envelope.observation_id {
        return Err("INVALID_OBSERVATION_ID");
    }
    if !bounded(&envelope.source_id, 256)
        || !bounded(&envelope.source_epoch, 256)
        || !bounded(&envelope.native_event, 128)
        || !bounded(&envelope.adapter_id, 64)
    {
        return Err("INVALID_HEADER");
    }
    if envelope
        .source_sequence
        .as_deref()
        .is_some_and(|s| parse_cursor(s).is_none())
    {
        return Err("INVALID_SOURCE_SEQUENCE");
    }
    let size = serde_json::to_vec(envelope).map_or(usize::MAX, |b| b.len());
    if size > OBSERVATION_MAX_BYTES {
        return Err("OBSERVATION_TOO_LARGE");
    }
    Ok(id)
}

fn insert_observation(
    tx: &Transaction<'_>,
    row: &ObservationRow,
    delivery: Delivery,
    now_ms: i64,
) -> Result<i64, JournalError> {
    tx.execute(
        "INSERT INTO observations (observation_id, source_id, source_epoch, source_sequence,
           native_event, captured_wall_ms, received_wall_ms, payload_version, payload_json,
           canonical, delivery, sequence_meaning)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?11, ?8, 1, ?9, ?10)",
        params![
            row.observation_id,
            row.source_id,
            row.source_epoch,
            row.source_sequence,
            row.native_event,
            row.captured_wall_ms,
            now_ms,
            row.payload_json,
            enum_text(&delivery),
            row.sequence_meaning.as_ref().and_then(enum_text),
            JOURNAL_PAYLOAD_VERSION,
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

fn insert_assignments(
    tx: &Transaction<'_>,
    assignments: &[Assignment],
    cursor: i64,
) -> Result<(), JournalError> {
    for assignment in assignments {
        tx.execute(
            "INSERT INTO identity_assignments (native_key, entity, canonical_id, ingest_seq)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                assignment.native_key,
                assignment.entity.as_str(),
                assignment.id,
                cursor
            ],
        )?;
    }
    Ok(())
}

fn insert_facts(tx: &Transaction<'_>, facts: &[ResolvedFact], cursor: i64) -> Result<(), JournalError> {
    for fact in facts {
        let json = serde_json::to_string(fact).map_err(|e| JournalError::Invalid {
            detail: e.to_string(),
        })?;
        tx.execute(
            "INSERT INTO facts (fact_id, observation_id, fact_index, ingest_seq, kind, session_id, fact_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                fact.fact_id,
                fact.observation_id,
                fact.fact_index,
                cursor,
                enum_text(&fact.payload.kind()),
                fact.refs.session_id,
                json
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn diagnostic(
    tx: &Transaction<'_>,
    cursor: Option<i64>,
    observation_id: Option<&str>,
    code: &str,
    detail: &str,
    now_ms: i64,
) -> Result<(), JournalError> {
    tx.execute(
        "INSERT INTO admission_diagnostics (ingest_seq, observation_id, code, detail, recorded_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![cursor, observation_id, code, detail, now_ms],
    )?;
    tx.execute(
        "DELETE FROM admission_diagnostics WHERE id <= (SELECT MAX(id) FROM admission_diagnostics) - ?1",
        params![DIAGNOSTICS_KEPT],
    )?;
    Ok(())
}

fn materialize_output(
    tx: &Transaction<'_>,
    state: &CanonicalState,
    output: &ReduceOutput,
) -> Result<(), JournalError> {
    for row in materialize::changed_rows(state, &output.changed) {
        materialize::upsert(tx, &row)?;
    }
    Ok(())
}

/// What one admitted step changed, merged across a batch.
#[derive(Default)]
struct Accumulated {
    cursor: Option<i64>,
    sessions: Vec<String>,
    attention: Vec<String>,
    outbox: Vec<String>,
    entries: u64,
}

impl Accumulated {
    fn add(&mut self, cursor: i64, output: &ReduceOutput) {
        self.cursor = Some(cursor);
        self.entries += 1;
        for id in &output.changed.sessions {
            if !self.sessions.contains(id) {
                self.sessions.push(id.clone());
            }
        }
        for id in &output.changed.attention {
            if !self.attention.contains(id) {
                self.attention.push(id.clone());
            }
        }
        self.outbox.extend(output.new_outbox.iter().cloned());
    }

    fn change(&self) -> Option<Change> {
        self.cursor.map(|cursor| Change {
            cursor,
            session_ids: self.sessions.clone(),
            attention_ids: self.attention.clone(),
        })
    }
}

fn category_title(category: &AttentionCategory) -> &'static str {
    match category {
        AttentionCategory::InputRequired => "needs input",
        AttentionCategory::ApprovalRequired => "needs approval",
        AttentionCategory::TurnComplete => "finished a turn",
        AttentionCategory::Error => "stopped with an error",
        AttentionCategory::Blocked => "is blocked",
        AttentionCategory::HandoffReady => "is ready to hand off",
        AttentionCategory::OwnerDecisionRequired => "needs a decision",
    }
}

/// The notification intent of a PENDING outbox record (identifiers and a
/// bounded label only; never provider text beyond a native summary label).
pub(crate) fn intent_for(state: &CanonicalState, outbox: &OutboxRecord) -> Option<NotificationIntent> {
    let item = state.attention.get(&outbox.attention_id)?;
    let session = state.sessions.get(&item.session_id)?;
    let name = session
        .inventory
        .as_ref()
        .and_then(|i| i.row.as_ref())
        .and_then(|r| r.display_name.clone())
        .or_else(|| session.display_name.clone())
        .unwrap_or_else(|| session.native_session_id.chars().take(8).collect());
    Some(NotificationIntent {
        request_id: outbox.request_id.clone(),
        attention_id: item.id.clone(),
        session_id: item.session_id.clone(),
        title: format!("{name} {}", category_title(&item.category)),
        body: item.summary.clone().unwrap_or_else(|| name.clone()),
    })
}

/// One canonical admission step inside an open transaction.
#[allow(clippy::too_many_arguments)]
fn admit_step(
    tx: &Transaction<'_>,
    engine: &mut Engine,
    resolver: &mut Resolver<'_>,
    endpoint_id: &str,
    row: &ObservationRow,
    drafts: &[NativeFactDraft],
    prebuilt: Vec<ResolvedFact>,
    unsupported: Option<&str>,
    delivery: Delivery,
    now_ms: i64,
    #[cfg(feature = "qualification")] armed: Option<CrashPoint>,
) -> Result<(i64, ReduceOutput, u64), JournalError> {
    let cursor = insert_observation(tx, row, delivery, now_ms)?;
    #[cfg(feature = "qualification")]
    crash::hit(armed, CrashPoint::InTransaction);
    let (mut facts, unresolved) = resolver.resolve(&engine.state, &row.observation_id, drafts);
    let offset = facts.len() as u32;
    for (index, mut fact) in prebuilt.into_iter().enumerate() {
        fact.fact_index = offset + index as u32;
        facts.push(fact);
    }
    insert_assignments(tx, &resolver.drain_new(), cursor)?;
    insert_facts(tx, &facts, cursor)?;
    if let Some(reason) = unsupported {
        diagnostic(tx, Some(cursor), Some(&row.observation_id), "UNSUPPORTED_EVENT", reason, now_ms)?;
    }
    for item in &unresolved {
        diagnostic(
            tx,
            Some(cursor),
            Some(&row.observation_id),
            "UNRESOLVED_DRAFT",
            &format!("{}#{}: {}", enum_text(&item.kind).unwrap_or_default(), item.fact_index, item.reason),
            now_ms,
        )?;
    }
    #[cfg(feature = "qualification")]
    crash::hit(armed, CrashPoint::AfterFacts);
    let entry = JournalEntry {
        cursor,
        payload_version: JOURNAL_PAYLOAD_VERSION,
        endpoint_id: endpoint_id.to_owned(),
        observation_id: row.observation_id.clone(),
        source_id: row.source_id.clone(),
        source_epoch: row.source_epoch.clone(),
        source_sequence: row.source_sequence.clone(),
        sequence_meaning: row.sequence_meaning,
        captured_wall_ms: row.captured_wall_ms,
        delivery,
        facts,
    };
    let output = engine.apply(&entry);
    materialize_output(tx, &engine.state, &output)?;
    #[cfg(feature = "qualification")]
    crash::hit(armed, CrashPoint::AfterReduce);
    Ok((cursor, output, unresolved.len() as u64))
}

/// Writes a periodic checkpoint once enough entries accumulated.
fn maybe_checkpoint(
    tx: &Transaction<'_>,
    state: &CanonicalState,
    since: &mut u64,
    entries: u64,
    now_ms: i64,
) -> Result<(), JournalError> {
    *since += entries;
    if *since >= CHECKPOINT_INTERVAL {
        write_checkpoint(tx, state, "PERIODIC", now_ms)?;
        *since = 0;
    }
    Ok(())
}

fn write_checkpoint(
    tx: &Transaction<'_>,
    state: &CanonicalState,
    origin: &str,
    now_ms: i64,
) -> Result<String, JournalError> {
    let json = serde_json::to_string(state).map_err(|e| JournalError::Invalid {
        detail: e.to_string(),
    })?;
    let sha = sha256_hex(json.as_bytes());
    tx.execute(
        "INSERT INTO projection_checkpoints (reducer_version, schema_version, through_cursor,
           state_json, state_sha256, origin, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            REDUCER_VERSION,
            crate::SCHEMA_VERSION,
            state.through_cursor,
            json,
            sha,
            origin,
            now_ms
        ],
    )?;
    tx.execute(
        "DELETE FROM projection_checkpoints WHERE id <= (SELECT MAX(id) FROM projection_checkpoints) - ?1",
        params![CHECKPOINTS_KEPT],
    )?;
    Ok(sha)
}

/// Reads one canonical entry's header and facts.
fn entries_after(conn: &rusqlite::Connection, after: i64, endpoint_id: &str) -> Result<Vec<JournalEntry>, JournalError> {
    let mut statement = conn.prepare(
        "SELECT ingest_seq, observation_id, source_id, source_epoch, source_sequence,
                sequence_meaning, captured_wall_ms, delivery, payload_version
           FROM observations WHERE canonical = 1 AND ingest_seq > ?1 ORDER BY ingest_seq",
    )?;
    let mut entries: Vec<JournalEntry> = statement
        .query_map(params![after], |row| {
            let meaning: Option<String> = row.get(5)?;
            let delivery: Option<String> = row.get(7)?;
            Ok(JournalEntry {
                cursor: row.get(0)?,
                payload_version: row.get(8)?,
                endpoint_id: endpoint_id.to_owned(),
                observation_id: row.get(1)?,
                source_id: row.get(2)?,
                source_epoch: row.get(3)?,
                source_sequence: row.get(4)?,
                sequence_meaning: meaning
                    .and_then(|m| serde_json::from_value(Value::String(m)).ok()),
                captured_wall_ms: row.get(6)?,
                delivery: delivery
                    .and_then(|d| serde_json::from_value(Value::String(d)).ok())
                    .unwrap_or(Delivery::Live),
                facts: Vec::new(),
            })
        })?
        .collect::<Result<_, _>>()?;
    drop(statement);
    if let Some(entry) = entries.iter().find(|entry| entry.payload_version == 0 || entry.payload_version > JOURNAL_PAYLOAD_VERSION) {
        return Err(JournalError::Invalid {
            detail: format!("unsupported journal payload version {} at cursor {}", entry.payload_version, entry.cursor),
        });
    }
    let mut statement = conn.prepare(
        "SELECT ingest_seq, fact_json FROM facts WHERE ingest_seq > ?1 ORDER BY ingest_seq, fact_index",
    )?;
    let mut rows = statement.query(params![after])?;
    let mut position = 0usize;
    while let Some(row) = rows.next()? {
        let cursor: i64 = row.get(0)?;
        let json: String = row.get(1)?;
        let fact: ResolvedFact = serde_json::from_str(&json).map_err(|e| JournalError::Invalid {
            detail: format!("fact at {cursor}: {e}"),
        })?;
        while entries.get(position).is_some_and(|e| e.cursor < cursor) {
            position += 1;
        }
        match entries.get_mut(position) {
            Some(entry) if entry.cursor == cursor => entry.facts.push(fact),
            _ => {
                return Err(JournalError::Invalid {
                    detail: format!("fact at {cursor} has no canonical observation"),
                });
            }
        }
    }
    Ok(entries)
}

/// The newest verified checkpoint and the reducer version that wrote it,
/// refusing one from a newer reducer. One from an earlier reducer is read
/// through its version's representation (`upgrade_json`).
fn latest_checkpoint(conn: &rusqlite::Connection) -> Result<Option<(CanonicalState, u32)>, JournalError> {
    let row: Option<(u32, String, String)> = conn
        .query_row(
            "SELECT reducer_version, state_json, state_sha256 FROM projection_checkpoints
              ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((version, json, sha)) = row else {
        return Ok(None);
    };
    if version > REDUCER_VERSION {
        return Err(JournalError::SchemaTooNew { found: version });
    }
    if sha256_hex(json.as_bytes()) != sha {
        return Err(JournalError::Invalid {
            detail: "checkpoint digest mismatch".into(),
        });
    }
    let invalid = |e: serde_json::Error| JournalError::Invalid {
        detail: format!("checkpoint: {e}"),
    };
    let mut value: Value = serde_json::from_str(&json).map_err(invalid)?;
    upgrade_json(version, &mut value);
    serde_json::from_value(value).map(|state| Some((state, version))).map_err(invalid)
}

/// Rewrites a checkpoint from an earlier reducer into this reducer's
/// representation. Reducer 1 recorded only whether a wait owner decision's
/// episode held positives without a causal point; this reducer records how
/// many the decision covered. `true` becomes 1, the fewest it can have
/// covered, so a later such positive is never taken as handled.
fn upgrade_json(version: u32, state: &mut Value) {
    if version >= 2 {
        return;
    }
    let Some(waits) = state.get_mut("waits").and_then(Value::as_object_mut) else { return };
    let decisions = waits
        .values_mut()
        .filter_map(|wait| wait.get_mut("ownerDecisions").and_then(Value::as_array_mut))
        .flatten();
    for decision in decisions {
        if let Some(held) = decision.get("unordered").and_then(Value::as_bool) {
            decision["unordered"] = Value::from(u32::from(held));
        }
    }
}

/// A checkpoint from before reducer 3 holds an execution's attach values,
/// a session's link and an input's origin as values, not the reports they
/// came from, which this reducer derives them from (D-0010). The journal
/// holds those reports: each fact at or before the checkpoint's cursor is
/// read back into the sets exactly as reducing it records it
/// (`record_evidence`), so the sets do not depend on where the checkpoint
/// sits. A record no fact reported keeps the values the checkpoint holds
/// (an M0 baseline's).
fn rebuild_evidence(conn: &rusqlite::Connection, state: &mut CanonicalState) -> Result<(), JournalError> {
    let mut statement =
        conn.prepare("SELECT fact_json FROM facts WHERE ingest_seq <= ?1 ORDER BY ingest_seq, fact_index")?;
    let facts = statement
        .query_map(params![state.through_cursor], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for json in facts {
        let fact: ResolvedFact = serde_json::from_str(&json).map_err(|e| JournalError::Invalid {
            detail: format!("journal fact: {e}"),
        })?;
        threadspace_state_engine::record_evidence(state, &fact);
    }
    Ok(())
}

/// All retained canonical facts through one cursor, for an explicit reducer
/// upgrade. No inferred projection fields substitute for missing facts.
fn facts_through(conn: &rusqlite::Connection, cursor: i64) -> Result<Vec<ResolvedFact>, JournalError> {
    let mut statement = conn.prepare("SELECT fact_json FROM facts WHERE ingest_seq <= ?1 ORDER BY ingest_seq, fact_index")?;
    let rows = statement.query_map(params![cursor], |row| row.get::<_, String>(0))?;
    rows.map(|row| {
        serde_json::from_str(&row?).map_err(|error| JournalError::Invalid {
            detail: format!("ownership upgrade fact: {error}"),
        })
    }).collect()
}

/// A wait owner decision an earlier reducer recorded after its checkpoint,
/// recovered by this reducer, read as that reducer recorded it, which is
/// how `upgrade_json` reads the decisions in its checkpoint: reducer 1 kept
/// only whether a decision covered positives without a causal point, read
/// as 1.
fn read_as_recorded(version: u32, decision: &mut WaitOwnerDecision) {
    if version < 2 {
        decision.unordered = decision.unordered.min(1);
    }
}

pub(crate) fn load_index(conn: &rusqlite::Connection) -> Result<IdentityIndex, JournalError> {
    let mut index = IdentityIndex::default();
    let mut statement = conn.prepare("SELECT native_key, canonical_id FROM identity_assignments")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        index.insert(row.get(0)?, row.get(1)?);
    }
    Ok(index)
}

/// Rebuilds the engine: newest verified checkpoint, then every later entry.
/// A checkpoint from an earlier reducer is then re-derived under this one;
/// `true` says so, and opening the store persists the result
/// (`persist_upgrade`).
///
/// The entries after an earlier reducer's checkpoint were admitted by that
/// reducer, which committed their notification work with them. They are
/// replayed only to recover the state (`Engine::recover`), the owner
/// decisions they record are read as that reducer recorded them
/// (`read_as_recorded`), the result is rebased onto what it committed
/// (`keep_committed`), and only then is the state re-derived. So where the
/// checkpoint sits changes nothing: a committed intent keeps its state, and
/// eligibility first found by this reducer is held, whether it appears
/// during the replay or the re-derivation.
pub(crate) fn load_engine(conn: &rusqlite::Connection, endpoint_id: &str) -> Result<(Engine, u64, bool), JournalError> {
    let (mut state, version) = latest_checkpoint(conn)?.unwrap_or_else(|| (Engine::empty().state, REDUCER_VERSION));
    if version < 3 {
        rebuild_evidence(conn, &mut state)?;
    }
    let mut engine = Engine::new(state);
    let after = engine.state.through_cursor;
    let entries = entries_after(conn, after, endpoint_id)?;
    let replayed = entries.len() as u64;
    let upgraded = version < REDUCER_VERSION;
    if upgraded {
        // An older verified checkpoint may precede an already committed
        // current-version suffix. Recover only the historical prefix under
        // old authority; its first v2 observation fixes the original
        // transition even when that observation contains no facts.
        let transition = entries.iter().position(|entry| entry.payload_version >= 2).unwrap_or(entries.len());
        let decisions = |state: &CanonicalState| -> std::collections::BTreeSet<String> {
            state.waits.values().flat_map(|w| &w.owner_decisions).map(|d| d.command_id.clone()).collect()
        };
        let checkpointed = decisions(&engine.state);
        for entry in &entries[..transition] {
            engine.recover(entry);
        }
        let mut state = engine.state;
        let recovered = state.waits.values_mut().flat_map(|w| w.owner_decisions.iter_mut());
        for decision in recovered.filter(|d| !checkpointed.contains(&d.command_id)) {
            read_as_recorded(version, decision);
        }
        keep_committed(conn, &mut state)?;
        engine = Engine::new(state);
        let facts = facts_through(conn, engine.state.through_cursor)?;
        engine.upgrade(endpoint_id, &facts).map_err(|detail| JournalError::Invalid { detail })?;
        keep_committed_revisions(conn, &mut engine.state)?;
        for entry in &entries[transition..] {
            engine.apply(entry);
        }
    } else {
        for entry in &entries {
            engine.apply(entry);
        }
    }
    Ok((engine, replayed, upgraded))
}

/// Rebases a recovered state onto what the earlier reducer committed
/// through the journal's last entry (the materialized rows), so the
/// re-derivation starts where it would from a checkpoint taken there:
///
/// - an intent with a row takes the record the row holds: its state,
///   detail, times and revision. One without a row was first derived by
///   this reducer; it is dropped, and the re-derivation creates it as the
///   upgrade's own;
/// - an attention item takes its committed resolution time, and keeps its
///   committed revision unless its row now differs, which makes it a change
///   of the upgrade, at its cursor. The re-derivation then decides from the
///   item's evidence whether it is resolved: it keeps that time while the
///   item stays resolved and clears it when it does not.
fn keep_committed(conn: &rusqlite::Connection, state: &mut CanonicalState) -> Result<(), JournalError> {
    let invalid = |table: &str, key: &str| JournalError::Invalid {
        detail: format!("committed {table} row {key}"),
    };
    let integer = |row: &materialize::Named, column: &str| match row.get(column) {
        Some(Sql::Integer(value)) => Some(*value),
        _ => None,
    };
    let cursor = state.through_cursor;
    let attention = materialize::committed_rows(conn, "attention_items")?;
    for item in state.attention.values_mut() {
        let committed = attention.get(&item.id);
        item.revision = match committed {
            Some(row) => integer(row, "revision").ok_or_else(|| invalid("attention", &item.id))?,
            None => cursor,
        };
        if let Some(row) = committed {
            item.resolved_at_ms = nullable_integer(row, "resolved_at_ms").ok_or_else(|| invalid("attention", &item.id))?;
            // Outbox dispositions below are rebased onto their committed
            // values. Preserve their corresponding display state too: an
            // already suppressed intent need not repeat the transition
            // that originally changed PENDING to NOT_REQUESTED.
            item.notification_state = match row.get("notification_state") {
                Some(Sql::Text(name)) => serde_json::from_value(Value::String(name.clone()))
                    .map_err(|_| invalid("attention notification_state", &item.id))?,
                _ => return Err(invalid("attention notification_state", &item.id)),
            };
        }
        if committed != Some(&materialize::attention_row(item).named()) {
            item.revision = cursor;
        }
    }
    let outbox = materialize::committed_rows(conn, "notification_outbox")?;
    state.outbox.retain(|request_id, _| outbox.contains_key(request_id));
    for record in state.outbox.values_mut() {
        let row = &outbox[&record.request_id];
        let fail = || invalid("outbox", &record.request_id);
        record.state = match row.get("state") {
            Some(Sql::Text(name)) => serde_json::from_value(Value::String(name.clone())).map_err(|_| fail())?,
            _ => return Err(fail()),
        };
        record.detail = match row.get("outcome_detail") {
            Some(Sql::Text(detail)) => Some(detail.clone()),
            Some(Sql::Null) => None,
            _ => return Err(fail()),
        };
        record.created_at_ms = integer(row, "created_at_ms").ok_or_else(fail)?;
        record.updated_at_ms = integer(row, "updated_at_ms").ok_or_else(fail)?;
        record.revision = integer(row, "revision").ok_or_else(fail)?;
    }
    Ok(())
}

/// The revision a record takes after an upgrade: its committed row's, when
/// the row the upgrade materializes for it is that row; otherwise the
/// upgrade's cursor.
fn committed_revision(
    committed: &std::collections::BTreeMap<String, materialize::Named>,
    id: &str,
    cursor: i64,
    row_at: impl Fn(i64) -> materialize::Row,
) -> i64 {
    match committed.get(id).map(|row| (row, row.get("revision"))) {
        Some((row, Some(Sql::Integer(revision)))) if row_at(*revision).named() == *row => *revision,
        _ => cursor,
    }
}

/// After the re-derivation, a record whose materialized row the upgrade
/// left as the earlier reducer committed it keeps that row's revision, and
/// one whose row it changed takes the upgrade's cursor. Where in the replay
/// a value changed is then no part of the result, so the upgraded state does
/// not depend on where the checkpoint sits (D-0007 §6, D-0010). Attention
/// and outbox follow `keep_committed`; a binding keeps its proof revision.
fn keep_committed_revisions(conn: &rusqlite::Connection, state: &mut CanonicalState) -> Result<(), JournalError> {
    let cursor = state.through_cursor;
    macro_rules! rebase {
        ($records:ident, $table:literal, |$record:ident| $row:expr) => {{
            let committed = materialize::committed_rows(conn, $table)?;
            let revisions: Vec<(String, i64)> = state
                .$records
                .iter()
                .map(|(id, record)| {
                    let revision = committed_revision(&committed, id, cursor, |revision| {
                        let mut probe = record.clone();
                        probe.revision = revision;
                        let $record = &probe;
                        $row
                    });
                    (id.clone(), revision)
                })
                .collect();
            for (id, revision) in revisions {
                if let Some(record) = state.$records.get_mut(&id) {
                    record.revision = revision;
                }
            }
        }};
    }
    rebase!(sessions, "sessions", |r| materialize::session_row(state, r));
    rebase!(processes, "process_incarnations", |r| materialize::process_row(r));
    rebase!(actors, "actors", |r| materialize::actor_row(r));
    rebase!(executions, "executions", |r| materialize::execution_rows(r, materialize::has_valid_binding(state, &r.id))
        .into_iter()
        .find(|row| row.table == "executions")
        .expect("an execution row"));
    rebase!(surfaces, "source_surfaces", |r| materialize::surface_row(r));
    rebase!(turns, "turns", |r| materialize::turn_row(r));
    rebase!(inputs, "inputs", |r| materialize::input_row(r));
    rebase!(activities, "activities", |r| materialize::activity_row(r));
    rebase!(waits, "wait_scopes", |r| materialize::wait_row(r));
    rebase!(coverage, "source_coverage", |r| materialize::coverage_row(r));
    Ok(())
}

/// A committed nullable integer: `Some(None)` for NULL, and `None` when the
/// column is missing or holds any other type.
fn nullable_integer(row: &materialize::Named, column: &str) -> Option<Option<i64>> {
    match row.get(column) {
        Some(Sql::Integer(value)) => Some(Some(*value)),
        Some(Sql::Null) => Some(None),
        _ => None,
    }
}

/// Persists a state upgraded from an earlier reducer's checkpoint: every
/// materialized row rewritten from it and a checkpoint at this reducer, in
/// one transaction.
pub(crate) fn persist_upgrade(
    conn: &mut rusqlite::Connection,
    state: &CanonicalState,
    now_ms: i64,
) -> Result<(), JournalError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    initial_checkpoint(&tx, state, "REDUCER_UPGRADE", now_ms)?;
    tx.commit()?;
    Ok(())
}

/// Digests describing a journal and the state it reproduces (M1 evidence).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayDigest {
    pub reducer_version: u32,
    pub schema_version: u32,
    pub entries: u64,
    pub facts: u64,
    pub first_cursor: Option<i64>,
    pub last_cursor: Option<i64>,
    pub journal_sha256: String,
    pub checkpoint_sha256: Option<String>,
    pub checkpoint_cursor: Option<i64>,
    pub state_sha256: String,
    pub projection_sha256: String,
    pub tables_sha256: String,
    pub semantic_sha256: String,
}

impl Journal {
    fn reload_engine(&mut self) {
        match load_engine(&self.conn, &self.endpoint_id).and_then(|(engine, _, _)| {
            load_index(&self.conn).map(|index| (engine, index))
        }) {
            Ok((engine, index)) => {
                self.engine = engine;
                self.index = index;
            }
            Err(_) => {
                // The store is unreadable: refuse further admission rather
                // than reduce on state the journal does not hold.
                self.poisoned = true;
            }
        }
    }

    fn guard(&self) -> Result<(), JournalError> {
        if self.poisoned {
            return Err(JournalError::Invalid {
                detail: "canonical state could not be rebuilt from the store".into(),
            });
        }
        Ok(())
    }

    /// The reducer's current canonical state.
    pub fn canonical_state(&self) -> &CanonicalState {
        &self.engine.state
    }

    /// Admits captured envelopes in one transaction (SPEC §8.2, §8.4).
    pub fn admit_batch(
        &mut self,
        batch: &[EnvelopeAdmission<'_>],
        delivery: Delivery,
        now_ms: i64,
    ) -> Result<BatchOutcome, JournalError> {
        self.guard()?;
        #[cfg(feature = "qualification")]
        let armed = self.crash.next_admission();
        #[cfg(feature = "qualification")]
        crash::hit(armed, CrashPoint::BeforeTransaction);
        let result = self.admit_batch_inner(
            batch,
            delivery,
            now_ms,
            #[cfg(feature = "qualification")]
            armed,
        );
        if result.is_err() {
            self.reload_engine();
        }
        #[cfg(feature = "qualification")]
        if result.is_ok() {
            crash::hit(armed, CrashPoint::AfterCommitBeforeReceipt);
        }
        result
    }

    fn admit_batch_inner(
        &mut self,
        batch: &[EnvelopeAdmission<'_>],
        delivery: Delivery,
        now_ms: i64,
        #[cfg(feature = "qualification")] armed: Option<CrashPoint>,
    ) -> Result<BatchOutcome, JournalError> {
        let mut outcome = BatchOutcome::default();
        let mut accumulated = Accumulated::default();
        let mut all_assignments = Vec::new();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut resolver = Resolver::new(&self.index, self.allocator.as_mut(), &self.endpoint_id);
            for item in batch {
                let envelope = item.envelope;
                let observation_id = match validate_envelope(envelope) {
                    Ok(id) => id,
                    Err(reason) => {
                        outcome.records.push(RecordOutcome {
                            observation_id: envelope.observation_id.clone(),
                            status: RecordStatus::NotAccepted,
                            cursor: None,
                            reason: Some(reason.into()),
                        });
                        continue;
                    }
                };
                let mut stored = envelope.clone();
                stored.observation_id.clone_from(&observation_id);
                stored.payload = item.normalized.retained.clone();
                let payload_json = serde_json::to_string(&stored).map_err(|e| JournalError::Invalid {
                    detail: e.to_string(),
                })?;
                let existing: Option<(i64, String)> = tx
                    .query_row(
                        "SELECT ingest_seq, payload_json FROM observations WHERE observation_id = ?1",
                        params![observation_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?;
                if let Some((cursor, previous)) = existing {
                    let (status, reason) = if previous == payload_json {
                        (RecordStatus::AlreadyCommitted, None)
                    } else {
                        (RecordStatus::NotAccepted, Some("OBSERVATION_ID_CONFLICT".to_owned()))
                    };
                    outcome.records.push(RecordOutcome {
                        observation_id,
                        status,
                        cursor: Some(cursor),
                        reason,
                    });
                    continue;
                }
                let row = ObservationRow {
                    observation_id: observation_id.clone(),
                    source_id: envelope.source_id.clone(),
                    source_epoch: envelope.source_epoch.clone(),
                    source_sequence: envelope.source_sequence.clone(),
                    sequence_meaning: envelope.sequence_meaning,
                    native_event: envelope.native_event.clone(),
                    captured_wall_ms: envelope.captured_at.wall_time_ms,
                    payload_json,
                };
                let (cursor, output, unresolved) = admit_step(
                    &tx,
                    &mut self.engine,
                    &mut resolver,
                    &self.endpoint_id,
                    &row,
                    &item.normalized.drafts,
                    Vec::new(),
                    item.normalized.unsupported.as_deref(),
                    delivery,
                    now_ms,
                    #[cfg(feature = "qualification")]
                    armed,
                )?;
                outcome.unresolved += unresolved;
                outcome.unsupported += u64::from(item.normalized.unsupported.is_some());
                accumulated.add(cursor, &output);
                outcome.records.push(RecordOutcome {
                    observation_id,
                    status: RecordStatus::Committed,
                    cursor: Some(cursor),
                    reason: None,
                });
            }
            all_assignments.extend(resolver.into_assignments());
        }
        if let Some(cursor) = accumulated.cursor {
            meta_set(&tx, "applied_cursor", &format_cursor(cursor))?;
        }
        maybe_checkpoint(
            &tx,
            &self.engine.state,
            &mut self.entries_since_checkpoint,
            accumulated.entries,
            now_ms,
        )?;
        #[cfg(feature = "qualification")]
        let commit_begin = crate::latency::now_ns();
        tx.commit()?;
        #[cfg(feature = "qualification")]
        { outcome.commit_timing = crate::latency::finish(commit_begin); }
        self.index.apply(&all_assignments);
        outcome.notifications = accumulated
            .outbox
            .iter()
            .filter_map(|id| self.engine.state.outbox.get(id))
            // An item raised and ended within the batch suppressed its intent.
            .filter(|outbox| outbox.state == OutboxState::Pending)
            .filter_map(|outbox| intent_for(&self.engine.state, outbox))
            .collect();
        outcome.change = accumulated.change();
        Ok(outcome)
    }

    /// A fresh identifier from this journal's allocator.
    pub(crate) fn allocate_id(&mut self) -> String {
        self.allocator.allocate()
    }

    /// Admits drafts the companion itself produced (reconciliation, fixtures,
    /// route and notification records) as one canonical entry under
    /// `observation_id`. `extra` writes operational rows that commit with it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn admit_internal(
        &mut self,
        observation_id: String,
        source_id: &str,
        native_event: &str,
        payload: &Value,
        drafts: &[NativeFactDraft],
        prebuilt: Vec<ResolvedFact>,
        delivery: Delivery,
        captured_wall_ms: i64,
        now_ms: i64,
        extra: impl FnOnce(&Transaction<'_>, &str, i64, &CanonicalState) -> Result<(), JournalError>,
    ) -> Result<(i64, ReduceOutput), JournalError> {
        self.guard()?;
        let result = self.admit_internal_inner(
            observation_id,
            source_id,
            native_event,
            payload,
            drafts,
            prebuilt,
            delivery,
            captured_wall_ms,
            now_ms,
            extra,
        );
        if result.is_err() {
            self.reload_engine();
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn admit_internal_inner(
        &mut self,
        observation_id: String,
        source_id: &str,
        native_event: &str,
        payload: &Value,
        drafts: &[NativeFactDraft],
        prebuilt: Vec<ResolvedFact>,
        delivery: Delivery,
        captured_wall_ms: i64,
        now_ms: i64,
        extra: impl FnOnce(&Transaction<'_>, &str, i64, &CanonicalState) -> Result<(), JournalError>,
    ) -> Result<(i64, ReduceOutput), JournalError> {
        let row = ObservationRow {
            observation_id: observation_id.clone(),
            source_id: source_id.to_owned(),
            source_epoch: self.source_epoch.clone(),
            source_sequence: None,
            sequence_meaning: None,
            native_event: native_event.to_owned(),
            captured_wall_ms,
            payload_json: payload.to_string(),
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (cursor, output, assignments) = {
            let mut resolver = Resolver::new(&self.index, self.allocator.as_mut(), &self.endpoint_id);
            let (cursor, output, _) = admit_step(
                &tx,
                &mut self.engine,
                &mut resolver,
                &self.endpoint_id,
                &row,
                drafts,
                prebuilt,
                None,
                delivery,
                now_ms,
                #[cfg(feature = "qualification")]
                None,
            )?;
            (cursor, output, resolver.into_assignments())
        };
        extra(&tx, &observation_id, cursor, &self.engine.state)?;
        meta_set(&tx, "applied_cursor", &format_cursor(cursor))?;
        maybe_checkpoint(&tx, &self.engine.state, &mut self.entries_since_checkpoint, 1, now_ms)?;
        tx.commit()?;
        self.index.apply(&assignments);
        Ok((cursor, output))
    }

    /// Admits one owner command (SPEC §18.3). A committed command ID with the
    /// same payload returns its recorded receipt even after the target's
    /// revision advanced; a reused ID with a different payload is a conflict;
    /// only a new command is checked against the item's current revision.
    pub fn admit_owner_command(
        &mut self,
        command_in: &OwnerCommand,
        now_ms: i64,
    ) -> Result<CommandOutcome, JournalError> {
        self.guard()?;
        let print = command::fingerprint(command_in);
        let existing: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT payload_fingerprint, result_json FROM attention_commands WHERE command_id = ?1",
                params![command_in.command_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((recorded, result_json)) = existing {
            // A command the M0 store committed keeps M0's fingerprint.
            if recorded != print && crate::baseline::m0_fingerprint(command_in).as_ref() != Some(&recorded) {
                return Err(JournalError::Conflict {
                    detail: format!(
                        "command {} was already used for a different payload",
                        command_in.command_id
                    ),
                });
            }
            let mut receipt: CommandReceipt =
                serde_json::from_str(&result_json).map_err(|e| JournalError::Invalid {
                    detail: e.to_string(),
                })?;
            receipt.status = ReceiptStatus::AlreadyCommitted;
            return Ok(CommandOutcome {
                receipt,
                change: None,
            });
        }
        let Some(item) = self.engine.state.attention.get(&command_in.attention_id) else {
            return Err(JournalError::NotFound {
                entity: "attention",
                id: command_in.attention_id.clone(),
            });
        };
        if let Some(expected) = command_in.expected_revision.as_deref()
            && parse_cursor(expected) != Some(item.revision)
        {
            return Err(JournalError::Conflict {
                detail: format!(
                    "attention {} is at revision {}, not {expected}",
                    item.id, item.revision
                ),
            });
        }
        let session_id = item.session_id.clone();
        let observation_id = self.allocate_id();
        let fact_id = self.allocate_id();
        let fact = command::fact(fact_id, &observation_id, &session_id, command_in, now_ms);
        let payload = serde_json::to_value(command_in).map_err(|e| JournalError::Invalid {
            detail: e.to_string(),
        })?;
        let action = match &command_in.action {
            OwnerAction::Acknowledge => "AcknowledgeAttention",
            OwnerAction::Resolve { .. } => "ResolveAttention",
            OwnerAction::Snooze { .. } => "SnoozeAttention",
        };
        let command_id = command_in.command_id.clone();
        let attention_id = command_in.attention_id.clone();
        let mut receipt_out = None;
        let (cursor, output) = self.admit_internal(
            observation_id,
            command::OWNER_SOURCE,
            "OWNER_COMMAND",
            &payload,
            &[],
            vec![fact],
            Delivery::Live,
            now_ms,
            now_ms,
            |tx, observation_id, cursor, state| {
                // A command that changed nothing leaves the item's revision.
                let revision = state.attention.get(&attention_id).map_or(cursor, |item| item.revision);
                let receipt = CommandReceipt {
                    command_id: command_id.clone(),
                    status: ReceiptStatus::Committed,
                    cursor: format_cursor(cursor),
                    target_revision: format_cursor(revision),
                };
                let result_json = serde_json::to_string(&receipt).map_err(|e| JournalError::Invalid {
                    detail: e.to_string(),
                })?;
                tx.execute(
                    "INSERT INTO attention_commands (command_id, attention_id, action, payload_json,
                       payload_fingerprint, result_json, observation_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![command_id, attention_id, action, payload.to_string(), print, result_json, observation_id],
                )?;
                receipt_out = Some(receipt);
                Ok(())
            },
        )?;
        let receipt = receipt_out.ok_or_else(|| JournalError::Invalid {
            detail: "command receipt missing".into(),
        })?;
        let mut session_ids: Vec<String> = output.changed.sessions.iter().cloned().collect();
        if !session_ids.contains(&session_id) {
            session_ids.push(session_id);
        }
        Ok(CommandOutcome {
            receipt,
            change: Some(Change {
                cursor,
                session_ids,
                attention_ids: vec![command_in.attention_id.clone()],
            }),
        })
    }

    /// Records a capture coverage loss as a canonical gap fact: records the
    /// spool could not hold (one marker each) and spool records that expired
    /// unadmitted. A loss is a known gap, never a provider outcome.
    pub fn record_capture_loss(
        &mut self,
        dropped: &[String],
        expired: usize,
        now_ms: i64,
    ) -> Result<Change, JournalError> {
        use threadspace_contracts::canonical::fact::{EvidenceClass, FactPayload, NativeRefs};
        let mut reasons: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for marker in dropped {
            let reason = marker.split_once('.').map_or("unknown", |(_, r)| r);
            *reasons.entry(reason.to_owned()).or_default() += 1;
        }
        let summary: Vec<String> = reasons.iter().map(|(r, n)| format!("{r}={n}")).collect();
        let detail: String = format!("dropped {} [{}]; expired {expired}", dropped.len(), summary.join(","))
            .chars()
            .take(240)
            .collect();
        let draft = NativeFactDraft {
            refs: NativeRefs::default(),
            provenance: EvidenceClass::Derived,
            causal: None,
            payload: FactPayload::ObservationGapDetected {
                domain: "capture-spool".into(),
                detail: detail.clone(),
            },
        };
        let observation_id = self.allocate_id();
        let payload = serde_json::json!({ "dropped": dropped.len(), "expired": expired, "detail": detail });
        let (cursor, output) = self.admit_internal(
            observation_id,
            "capture.spool",
            "CAPTURE_LOSS_RECORDED",
            &payload,
            &[draft],
            Vec::new(),
            Delivery::Live,
            now_ms,
            now_ms,
            |_, _, _, _| Ok(()),
        )?;
        Ok(Change {
            cursor,
            session_ids: output.changed.sessions.into_iter().collect(),
            attention_ids: Vec::new(),
        })
    }

    /// Writes a checkpoint of the current state now.
    pub fn checkpoint(&mut self, origin: &str, now_ms: i64) -> Result<String, JournalError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let sha = write_checkpoint(&tx, &self.engine.state, origin, now_ms)?;
        tx.commit()?;
        self.entries_since_checkpoint = 0;
        Ok(sha)
    }

    /// Every canonical entry after `after`, in cursor order.
    pub fn journal_entries(&self, after: i64) -> Result<Vec<JournalEntry>, JournalError> {
        entries_after(&self.conn, after, &self.endpoint_id)
    }

    /// Exact digests of the admitted journal, the newest checkpoint, the
    /// replayed state and the materialized tables.
    pub fn replay_digest(&self) -> Result<ReplayDigest, JournalError> {
        let entries = entries_after(&self.conn, 0, &self.endpoint_id)?;
        let mut digest = JournalDigest::default();
        for entry in &entries {
            digest.add(entry);
        }
        let checkpoint: Option<(String, i64)> = self
            .conn
            .query_row(
                "SELECT state_sha256, through_cursor FROM projection_checkpoints ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (replayed, _, _) = load_engine(&self.conn, &self.endpoint_id)?;
        Ok(ReplayDigest {
            reducer_version: REDUCER_VERSION,
            schema_version: crate::SCHEMA_VERSION,
            entries: digest.entries,
            facts: digest.facts,
            first_cursor: digest.first_cursor,
            last_cursor: digest.last_cursor,
            journal_sha256: digest.finish(),
            checkpoint_sha256: checkpoint.as_ref().map(|c| c.0.clone()),
            checkpoint_cursor: checkpoint.map(|c| c.1),
            state_sha256: state_hash(&replayed.state),
            projection_sha256: materialize::hash_state(&replayed.state),
            tables_sha256: materialize::hash_tables(&self.conn)?,
            semantic_sha256: threadspace_state_engine::semantic::semantic_hash(&replayed.state),
        })
    }

    /// Replays every canonical entry from an empty state, ignoring
    /// checkpoints. Payload version 1 identifies history admitted before the
    /// explicit reducer-4 ownership transition. Its old HOST_READ effects
    /// must first be reconstructed to retain committed owner decisions and
    /// notification dispositions; the retained facts then withdraw any
    /// authority the new proof predicate cannot establish. The first v2
    /// entry is a permanent transition: later-delivered v1 history never
    /// switches the reducer back to its previous authority rules.
    pub fn replay_from_genesis(&self) -> Result<CanonicalState, JournalError> {
        let entries = entries_after(&self.conn, 0, &self.endpoint_id)?;
        let mut engine = Engine::empty();
        // Native-only old stores also need the explicit transition: it
        // records versioned checkpoint bookkeeping even when no HOST_READ
        // authority is withdrawn. A current v2 journal has no old prefix.
        if entries.first().is_some_and(|entry| entry.payload_version < 2) {
            engine.state.reducer_version = 3;
        }
        let mut retained = Vec::new();
        for entry in &entries {
            if engine.state.reducer_version < 4 && entry.payload_version >= 2 {
                engine.upgrade(&self.endpoint_id, &retained).map_err(|detail| JournalError::Invalid { detail })?;
            }
            engine.apply(entry);
            retained.extend(entry.facts.iter().cloned());
        }
        if engine.state.reducer_version < 4 {
            engine.upgrade(&self.endpoint_id, &retained).map_err(|detail| JournalError::Invalid { detail })?;
        }
        Ok(engine.state)
    }

    /// Per projection table, rows the state implies that SQLite lacks and
    /// rows SQLite holds that the state does not imply.
    pub fn projection_differences(&self) -> Result<Vec<materialize::TableDifference>, JournalError> {
        materialize::differences(&self.engine.state, &self.conn)
    }

    /// Admission diagnostics, newest first.
    pub fn admission_diagnostics(&self, limit: u32) -> Result<Vec<(String, String)>, JournalError> {
        let mut statement = self.conn.prepare(
            "SELECT code, detail FROM admission_diagnostics ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = statement
            .query_map(params![limit], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

pub(crate) fn meta_set(tx: &Transaction<'_>, key: &str, value: &str) -> Result<(), JournalError> {
    tx.execute(
        "INSERT INTO store_meta (key, value) VALUES (?1, ?2)
           ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub(crate) fn initial_checkpoint(
    tx: &Transaction<'_>,
    state: &CanonicalState,
    origin: &str,
    now_ms: i64,
) -> Result<(), JournalError> {
    for row in materialize::all_rows(state) {
        materialize::upsert(tx, &row)?;
    }
    write_checkpoint(tx, state, origin, now_ms)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Sql, materialize, nullable_integer};

    /// NULL is a committed absence, an integer its value; any other type,
    /// or no such column, is not a committed value at all.
    #[test]
    fn a_nullable_integer_reads_null_as_none() {
        let row = |value: Sql| materialize::Named::from([("resolved_at_ms", value)]);
        assert_eq!(nullable_integer(&row(Sql::Null), "resolved_at_ms"), Some(None));
        assert_eq!(
            nullable_integer(&row(Sql::Integer(1_791_000_000_040)), "resolved_at_ms"),
            Some(Some(1_791_000_000_040))
        );
        assert_eq!(nullable_integer(&row(Sql::Integer(0)), "resolved_at_ms"), Some(Some(0)));
        for malformed in [Sql::Text("40".into()), Sql::Real(40.0), Sql::Blob(vec![40])] {
            assert_eq!(nullable_integer(&row(malformed), "resolved_at_ms"), None);
        }
        assert_eq!(nullable_integer(&row(Sql::Null), "revision"), None);
    }
}
