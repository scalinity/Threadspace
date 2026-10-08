//! Materialized projections (SPEC §5.5, §9.2). Every projection row is built
//! from canonical state by one row builder, used both to write the row inside
//! the admission transaction and to hash the expected projection. Reading
//! the same columns back from SQLite and hashing them must give the same
//! digest: that is the check that the tables are exactly the reducer's state.

use rusqlite::types::Value as Sql;
use rusqlite::{Connection, Transaction};
use serde::Serialize;
use sha2::{Digest, Sha256};
use threadspace_contracts::canonical::records::{
    ActivityRecord, ActorRecord, ActorRelationRecord, AttentionRecord, CanonicalState,
    ExecutionRecord, InputRecord, NamespaceRecord, OutboxRecord, ProcessRecord, SessionRecord,
    SourceCoverage, SourceSurfaceRecord, SurfaceBindingRecord, TurnRecord, WaitScopeRecord,
};
use threadspace_state_engine::engine::Changed;

use crate::JournalError;

/// One projection table: its primary key and the columns this build owns.
pub(crate) struct Table {
    pub name: &'static str,
    pub key: &'static [&'static str],
    pub columns: &'static [&'static str],
}

/// Tables in foreign-key order.
pub(crate) const TABLES: &[Table] = &[
    Table {
        name: "provider_namespaces",
        key: &["id"],
        columns: &["id", "provider", "endpoint_id", "profile_ref"],
    },
    Table {
        name: "sessions",
        key: &["id"],
        columns: &[
            "id", "namespace_id", "native_session_id", "record_state", "display_name", "fixture",
            "revision", "provider_kind", "provider_status", "provider_waiting_for",
            "inventory_present", "execution_presence", "observation", "turn_state",
            "created_cursor",
        ],
    },
    Table {
        name: "process_incarnations",
        key: &["id"],
        columns: &[
            "id", "endpoint_id", "boot_id", "pid", "start_seconds", "start_microseconds",
            "executable_identity", "exited", "images_json",
        ],
    },
    Table {
        name: "actors",
        key: &["id"],
        columns: &["id", "session_id", "native_json", "role", "agent_types_json", "runs_ended", "revision"],
    },
    Table {
        name: "actor_relations",
        key: &["actor_id", "related_actor_id", "relation"],
        columns: &["actor_id", "related_actor_id", "relation"],
    },
    Table {
        name: "executions",
        key: &["id"],
        columns: &[
            "id", "session_id", "activation", "mode", "presence", "process_id", "device_number",
            "started_cursor", "ended_cursor", "end_reason", "surface_status", "actor_id",
            "activation_ref", "attached", "native_runtime_id",
        ],
    },
    Table {
        name: "execution_processes",
        key: &["execution_id", "process_id"],
        columns: &["execution_id", "process_id", "role"],
    },
    Table {
        name: "source_surfaces",
        key: &["id"],
        columns: &[
            "id", "endpoint_id", "surface_kind", "app_generation", "locator", "device_number",
            "surface_generation", "revision",
        ],
    },
    Table {
        name: "surface_bindings",
        key: &["id"],
        columns: &[
            "id", "session_id", "execution_id", "surface_kind", "native_locator", "proof",
            "revision", "valid", "process_id", "executable_identity", "device_number",
            "terminal_generation", "window_hint", "tab_hint", "evidence_observation", "proof_json",
            "invalidated_reason", "invalidated_cursor", "surface_id",
        ],
    },
    Table {
        name: "turns",
        key: &["id"],
        columns: &[
            "id", "session_id", "execution_id", "native_turn_id", "identity_kind", "state",
            "created_cursor", "actor_id", "owner_facing", "outcome_conflict", "output_ready",
            "revision",
        ],
    },
    Table {
        name: "inputs",
        key: &["id"],
        columns: &[
            "id", "session_id", "actor_id", "native_key", "origin", "submission_json",
            "active_turn_id", "accepted", "rejected", "started_turns_json", "revision",
        ],
    },
    Table {
        name: "activities",
        key: &["id"],
        columns: &[
            "id", "session_id", "actor_id", "turn_id", "native_occurrence_id",
            "tool_categories_json", "proposed", "started", "finished_json", "permission_checked",
            "revision",
        ],
    },
    Table {
        name: "attention_items",
        key: &["id"],
        columns: &[
            "id", "session_id", "turn_id", "category", "scope_kind", "scope_key", "priority",
            "summary", "created_by_observation", "created_at_ms", "acknowledged_at_ms",
            "resolved_at_ms", "notification_state", "revision", "actor_id", "resolution_reason",
            "snoozed_until_ms", "created_by_fact", "scope_json",
        ],
    },
    Table {
        name: "notification_outbox",
        key: &["request_id"],
        columns: &[
            "request_id", "attention_id", "state", "created_at_ms", "updated_at_ms",
            "outcome_detail", "revision",
        ],
    },
    Table {
        name: "wait_scopes",
        key: &["key"],
        columns: &[
            "key", "session_id", "actor_id", "execution_id", "turn_id", "category", "generation",
            "episodes_json", "revision",
        ],
    },
    Table {
        name: "source_coverage",
        key: &["key"],
        columns: &[
            "key", "source_id", "source_epoch", "meaning", "seen_json", "gaps_json",
            "reported_gaps_json", "revision",
        ],
    },
];

fn table(name: &str) -> &'static Table {
    TABLES
        .iter()
        .find(|t| t.name == name)
        .unwrap_or(&TABLES[0])
}

pub(crate) struct Row {
    pub table: &'static str,
    pub values: Vec<Sql>,
}

fn text(value: impl Into<String>) -> Sql {
    Sql::Text(value.into())
}

fn opt_text(value: Option<&str>) -> Sql {
    value.map_or(Sql::Null, |v| Sql::Text(v.to_owned()))
}

fn int(value: i64) -> Sql {
    Sql::Integer(value)
}

fn opt_int(value: Option<i64>) -> Sql {
    value.map_or(Sql::Null, Sql::Integer)
}

fn flag(value: bool) -> Sql {
    Sql::Integer(i64::from(value))
}

/// An enum's serialized name.
fn name<T: Serialize>(value: &T) -> Sql {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => Sql::Text(text),
        _ => Sql::Null,
    }
}

fn json<T: Serialize>(value: &T) -> Sql {
    Sql::Text(serde_json::to_string(value).unwrap_or_default())
}

fn short(native: &str) -> String {
    native.chars().take(8).collect()
}

pub(crate) fn namespace_row(r: &NamespaceRecord) -> Row {
    Row {
        table: "provider_namespaces",
        values: vec![text(&r.id), text(&r.provider), text(&r.endpoint_id), text(&r.profile_ref)],
    }
}

pub(crate) fn session_row(state: &CanonicalState, r: &SessionRecord) -> Row {
    let provider = state
        .namespaces
        .get(&r.namespace_id)
        .map_or("provider", |n| n.provider.as_str());
    let row = r.inventory.as_ref().and_then(|i| i.row.as_ref());
    let display = row
        .and_then(|row| row.display_name.clone())
        .or_else(|| r.display_name.clone())
        .unwrap_or_else(|| format!("{provider} {}", short(&r.native_session_id)));
    Row {
        table: "sessions",
        values: vec![
            text(&r.id),
            text(&r.namespace_id),
            text(&r.native_session_id),
            name(&r.record_state),
            text(display),
            flag(r.fixture),
            int(r.revision),
            opt_text(row.and_then(|row| row.kind.as_deref())),
            opt_text(row.and_then(|row| row.status.as_deref())),
            opt_text(row.and_then(|row| row.waiting_for.as_deref())),
            flag(r.inventory.as_ref().is_some_and(|i| i.present)),
            name(&r.execution_presence),
            name(&r.observation),
            name(&r.turn_state),
            int(r.created_cursor),
        ],
    }
}

pub(crate) fn process_row(r: &ProcessRecord) -> Row {
    let executable = r
        .current_executable
        .clone()
        .or_else(|| r.images.iter().next_back().map(|i| i.executable.clone()))
        .unwrap_or_default();
    Row {
        table: "process_incarnations",
        values: vec![
            text(&r.id),
            text(&r.key.endpoint_id),
            text(&r.key.boot_id),
            int(i64::from(r.key.pid)),
            int(r.key.start_seconds.parse::<i64>().unwrap_or(0)),
            int(i64::from(r.key.start_microseconds)),
            text(executable),
            flag(r.exited),
            json(&r.images),
        ],
    }
}

pub(crate) fn actor_row(r: &ActorRecord) -> Row {
    Row {
        table: "actors",
        values: vec![
            text(&r.id),
            text(&r.session_id),
            json(&r.native),
            name(&r.role),
            json(&r.agent_types),
            int(i64::from(r.runs_ended)),
            int(r.revision),
        ],
    }
}

pub(crate) fn relation_row(r: &ActorRelationRecord) -> Row {
    Row {
        table: "actor_relations",
        values: vec![text(&r.actor_id), text(&r.related_actor_id), name(&r.relation)],
    }
}

pub(crate) fn execution_rows(r: &ExecutionRecord, valid_binding: bool) -> Vec<Row> {
    let end_reason = r.end_reasons.iter().next().cloned().or_else(|| {
        (r.presence == threadspace_contracts::projection::ExecutionPresence::Ended)
            .then(|| "PROCESS_EXITED".to_owned())
    });
    let surface_status = if valid_binding {
        Some("BOUND".to_owned())
    } else {
        r.surface_status.clone()
    };
    let mut rows = vec![Row {
        table: "executions",
        values: vec![
            text(&r.id),
            text(&r.session_id),
            int(i64::try_from(r.activation).unwrap_or(i64::MAX)),
            r.mode.as_ref().map_or_else(|| text("unknown"), name),
            name(&r.presence),
            opt_text(r.process_id.as_deref()),
            opt_int(r.controlling_device.map(i64::from)),
            opt_int(r.started_cursor),
            opt_int(r.ended_cursor),
            opt_text(end_reason.as_deref()),
            opt_text(surface_status.as_deref()),
            text(&r.actor_id),
            text(&r.activation_ref),
            r.attached.as_ref().map_or(Sql::Null, name),
            opt_text(r.native_runtime_id.as_deref()),
        ],
    }];
    if let Some(process) = &r.process_id {
        rows.push(Row {
            table: "execution_processes",
            values: vec![text(&r.id), text(process), text("provider")],
        });
    }
    rows
}

pub(crate) fn surface_row(r: &SourceSurfaceRecord) -> Row {
    Row {
        table: "source_surfaces",
        values: vec![
            text(&r.id),
            text(&r.endpoint_id),
            text(&r.native.surface_kind),
            text(&r.native.app_generation),
            text(&r.native.locator),
            opt_int(r.native.device_number.map(i64::from)),
            text(&r.native.surface_generation),
            int(r.revision),
        ],
    }
}

pub(crate) fn binding_row(state: &CanonicalState, r: &SurfaceBindingRecord) -> Row {
    let surface = state.surfaces.get(&r.surface_id);
    let process = state
        .executions
        .get(&r.execution_id)
        .and_then(|e| e.process_id.clone());
    Row {
        table: "surface_bindings",
        values: vec![
            text(&r.id),
            text(&r.session_id),
            text(&r.execution_id),
            text(surface.map_or("unknown", |s| s.native.surface_kind.as_str())),
            text(surface.map_or("", |s| s.native.locator.as_str())),
            r.method.as_ref().map_or_else(|| text("UNPROVEN"), name),
            int(r.recorded_cursor.unwrap_or(r.created_cursor)),
            flag(r.valid),
            opt_text(process.as_deref()),
            opt_text(r.executable_identity.as_deref()),
            opt_int(surface.and_then(|s| s.native.device_number).map(i64::from)),
            opt_text(surface.map(|s| s.native.app_generation.as_str())),
            opt_int(r.window_hint),
            opt_int(r.tab_hint),
            opt_text(r.evidence_observation.as_deref()),
            text(r.proof.to_string()),
            opt_text(r.invalidation_reason.as_deref()),
            opt_int(r.invalidated_cursor),
            text(&r.surface_id),
        ],
    }
}

pub(crate) fn turn_row(r: &TurnRecord) -> Row {
    Row {
        table: "turns",
        values: vec![
            text(&r.id),
            text(&r.session_id),
            opt_text(r.execution_ids.iter().next().map(String::as_str)),
            opt_text(r.native_turn_id.as_deref()),
            name(&r.identity_kind),
            name(&r.state),
            int(r.created_cursor),
            text(&r.actor_id),
            flag(r.owner_facing),
            flag(r.outcome_conflict),
            flag(r.output_ready),
            int(r.revision),
        ],
    }
}

pub(crate) fn input_row(r: &InputRecord) -> Row {
    Row {
        table: "inputs",
        values: vec![
            text(&r.id),
            text(&r.session_id),
            text(&r.actor_id),
            text(&r.native_key),
            r.origin.as_ref().map_or(Sql::Null, name),
            r.submission.as_ref().map_or(Sql::Null, json),
            opt_text(r.active_turn_id.as_deref()),
            flag(r.acceptances.iter().any(|p| p.qualified())),
            flag(!r.rejections.is_empty()),
            json(&r.started_turns),
            int(r.revision),
        ],
    }
}

pub(crate) fn activity_row(r: &ActivityRecord) -> Row {
    Row {
        table: "activities",
        values: vec![
            text(&r.id),
            text(&r.session_id),
            text(&r.actor_id),
            opt_text(r.turn_id.as_deref()),
            text(&r.native_occurrence_id),
            json(&r.tool_categories),
            flag(r.proposed),
            flag(r.started),
            json(&r.finished),
            flag(r.permission_checked),
            int(r.revision),
        ],
    }
}

pub(crate) fn attention_row(r: &AttentionRecord) -> Row {
    Row {
        table: "attention_items",
        values: vec![
            text(&r.id),
            text(&r.session_id),
            opt_text(r.turn_id.as_deref()),
            name(&r.category),
            text(r.scope.scope_kind()),
            text(r.scope.scope_key()),
            int(i64::from(r.priority)),
            opt_text(r.summary.as_deref()),
            text(&r.created_by_observation),
            int(r.created_at_ms),
            opt_int(r.acknowledged_at_ms),
            opt_int(r.resolved_at_ms),
            name(&r.notification_state),
            int(r.revision),
            opt_text(r.actor_id.as_deref()),
            opt_text(r.resolution_reason.as_deref()),
            opt_int(r.snoozed_until_ms),
            text(&r.created_by_fact),
            json(&r.scope),
        ],
    }
}

pub(crate) fn outbox_row(r: &OutboxRecord) -> Row {
    Row {
        table: "notification_outbox",
        values: vec![
            text(&r.request_id),
            text(&r.attention_id),
            name(&r.state),
            int(r.created_at_ms),
            int(r.updated_at_ms),
            opt_text(r.detail.as_deref()),
            int(r.revision),
        ],
    }
}

pub(crate) fn wait_row(r: &WaitScopeRecord) -> Row {
    Row {
        table: "wait_scopes",
        values: vec![
            text(&r.key),
            text(&r.session_id),
            opt_text(r.actor_id.as_deref()),
            opt_text(r.execution_id.as_deref()),
            opt_text(r.turn_id.as_deref()),
            name(&r.category),
            opt_text(r.generation.as_deref()),
            json(&r.episodes),
            int(r.revision),
        ],
    }
}

pub(crate) fn coverage_row(r: &SourceCoverage) -> Row {
    Row {
        table: "source_coverage",
        values: vec![
            text(&r.key),
            text(&r.source_id),
            text(&r.source_epoch),
            name(&r.meaning),
            json(&r.seen),
            json(&r.gaps),
            json(&r.reported_gaps),
            int(r.revision),
        ],
    }
}

fn has_valid_binding(state: &CanonicalState, execution: &str) -> bool {
    state
        .bindings
        .values()
        .any(|b| b.execution_id == execution && b.valid)
}

/// Rows for the records a reduction changed, in foreign-key order.
pub(crate) fn changed_rows(state: &CanonicalState, changed: &Changed) -> Vec<Row> {
    let mut rows = Vec::new();
    let pick = |ids: &std::collections::BTreeSet<String>| ids.iter().cloned().collect::<Vec<_>>();
    for id in pick(&changed.namespaces) {
        rows.extend(state.namespaces.get(&id).map(namespace_row));
    }
    // A session's row shows its executions, bindings and turns: rewrite it
    // whenever any of them changed.
    let mut sessions = changed.sessions.clone();
    for id in changed.executions.iter().chain(&changed.bindings).chain(&changed.turns) {
        if let Some(session) = state
            .executions
            .get(id)
            .map(|e| &e.session_id)
            .or_else(|| state.bindings.get(id).map(|b| &b.session_id))
            .or_else(|| state.turns.get(id).map(|t| &t.session_id))
        {
            sessions.insert(session.clone());
        }
    }
    for id in pick(&sessions) {
        rows.extend(state.sessions.get(&id).map(|r| session_row(state, r)));
    }
    for id in pick(&changed.processes) {
        rows.extend(state.processes.get(&id).map(process_row));
    }
    for id in pick(&changed.actors) {
        rows.extend(state.actors.get(&id).map(actor_row));
    }
    if changed.relations {
        rows.extend(state.relations.iter().map(relation_row));
    }
    let mut executions = changed.executions.clone();
    for id in &changed.bindings {
        if let Some(binding) = state.bindings.get(id) {
            executions.insert(binding.execution_id.clone());
        }
    }
    for id in pick(&executions) {
        if let Some(r) = state.executions.get(&id) {
            rows.extend(execution_rows(r, has_valid_binding(state, &id)));
        }
    }
    for id in pick(&changed.surfaces) {
        rows.extend(state.surfaces.get(&id).map(surface_row));
    }
    // A binding's row shows its execution's process and its surface: rewrite
    // it when either changed, not only when the binding record did.
    let mut bindings = changed.bindings.clone();
    for binding in state.bindings.values() {
        if executions.contains(&binding.execution_id) || changed.surfaces.contains(&binding.surface_id) {
            bindings.insert(binding.id.clone());
        }
    }
    for id in pick(&bindings) {
        rows.extend(state.bindings.get(&id).map(|r| binding_row(state, r)));
    }
    for id in pick(&changed.turns) {
        rows.extend(state.turns.get(&id).map(turn_row));
    }
    for id in pick(&changed.inputs) {
        rows.extend(state.inputs.get(&id).map(input_row));
    }
    for id in pick(&changed.activities) {
        rows.extend(state.activities.get(&id).map(activity_row));
    }
    for id in pick(&changed.attention) {
        rows.extend(state.attention.get(&id).map(attention_row));
    }
    for id in pick(&changed.outbox) {
        rows.extend(state.outbox.get(&id).map(outbox_row));
    }
    for id in pick(&changed.waits) {
        rows.extend(state.waits.get(&id).map(wait_row));
    }
    for id in pick(&changed.coverage) {
        rows.extend(state.coverage.get(&id).map(coverage_row));
    }
    rows
}

/// Every projection row of a state, in foreign-key order.
pub(crate) fn all_rows(state: &CanonicalState) -> Vec<Row> {
    let mut rows = Vec::new();
    rows.extend(state.namespaces.values().map(namespace_row));
    rows.extend(state.sessions.values().map(|r| session_row(state, r)));
    rows.extend(state.processes.values().map(process_row));
    rows.extend(state.actors.values().map(actor_row));
    rows.extend(state.relations.iter().map(relation_row));
    for r in state.executions.values() {
        rows.extend(execution_rows(r, has_valid_binding(state, &r.id)));
    }
    rows.extend(state.surfaces.values().map(surface_row));
    rows.extend(state.bindings.values().map(|r| binding_row(state, r)));
    rows.extend(state.turns.values().map(turn_row));
    rows.extend(state.inputs.values().map(input_row));
    rows.extend(state.activities.values().map(activity_row));
    rows.extend(state.attention.values().map(attention_row));
    rows.extend(state.outbox.values().map(outbox_row));
    rows.extend(state.waits.values().map(wait_row));
    rows.extend(state.coverage.values().map(coverage_row));
    rows
}

pub(crate) fn upsert(tx: &Transaction<'_>, row: &Row) -> Result<(), JournalError> {
    let spec = table(row.table);
    let placeholders: Vec<String> = (1..=spec.columns.len()).map(|i| format!("?{i}")).collect();
    let updates: Vec<String> = spec
        .columns
        .iter()
        .filter(|c| !spec.key.contains(c))
        .map(|c| format!("{c} = excluded.{c}"))
        .collect();
    let conflict = if updates.is_empty() {
        "DO NOTHING".to_owned()
    } else {
        format!("DO UPDATE SET {}", updates.join(", "))
    };
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT({}) {conflict}",
        spec.name,
        spec.columns.join(", "),
        placeholders.join(", "),
        spec.key.join(", "),
    );
    let mut statement = tx.prepare_cached(&sql)?;
    statement.execute(rusqlite::params_from_iter(row.values.iter()))?;
    Ok(())
}

fn sql_json(value: &Sql) -> serde_json::Value {
    match value {
        Sql::Null => serde_json::Value::Null,
        Sql::Integer(i) => serde_json::Value::from(*i),
        Sql::Real(r) => serde_json::Value::from(*r),
        Sql::Text(t) => serde_json::Value::from(t.clone()),
        Sql::Blob(b) => serde_json::Value::from(b.clone()),
    }
}

/// Canonical text of rows grouped by table, each table's rows sorted by key.
fn digest(rows: impl Iterator<Item = (&'static str, Vec<Sql>)>) -> String {
    let mut by_table: std::collections::BTreeMap<&str, Vec<String>> = std::collections::BTreeMap::new();
    for (table, values) in rows {
        let line = serde_json::to_string(&values.iter().map(sql_json).collect::<Vec<_>>()).unwrap_or_default();
        by_table.entry(table).or_default().push(line);
    }
    let mut hasher = Sha256::new();
    for spec in TABLES {
        hasher.update(spec.name.as_bytes());
        hasher.update(b"\n");
        if let Some(lines) = by_table.get_mut(spec.name) {
            lines.sort();
            for line in lines.iter() {
                hasher.update(line.as_bytes());
                hasher.update(b"\n");
            }
        }
    }
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// One table's rows the state implies that SQLite lacks, and rows SQLite
/// holds that the state does not imply (canonical JSON of each row).
pub type TableDifference = (String, Vec<String>, Vec<String>);

/// Per-table differences between the state and SQLite, for diagnosing a
/// digest mismatch.
pub(crate) fn differences(
    state: &CanonicalState,
    conn: &Connection,
) -> Result<Vec<TableDifference>, JournalError> {
    let render = |values: &[Sql]| {
        serde_json::to_string(&values.iter().map(sql_json).collect::<Vec<_>>()).unwrap_or_default()
    };
    let mut expected: std::collections::BTreeMap<&str, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    for row in all_rows(state) {
        expected.entry(row.table).or_default().insert(render(&row.values));
    }
    let mut out = Vec::new();
    for spec in TABLES {
        let mut statement = conn.prepare(&format!("SELECT {} FROM {}", spec.columns.join(", "), spec.name))?;
        let mut cursor = statement.query([])?;
        let mut actual = std::collections::BTreeSet::new();
        while let Some(row) = cursor.next()? {
            let mut values = Vec::with_capacity(spec.columns.len());
            for index in 0..spec.columns.len() {
                values.push(row.get::<_, Sql>(index)?);
            }
            actual.insert(render(&values));
        }
        let wanted = expected.remove(spec.name).unwrap_or_default();
        let missing: Vec<String> = wanted.difference(&actual).cloned().collect();
        let extra: Vec<String> = actual.difference(&wanted).cloned().collect();
        if !missing.is_empty() || !extra.is_empty() {
            out.push((spec.name.to_owned(), missing, extra));
        }
    }
    Ok(out)
}

/// The projection digest the reducer's state implies.
pub(crate) fn hash_state(state: &CanonicalState) -> String {
    digest(all_rows(state).into_iter().map(|row| (row.table, row.values)))
}

/// The projection digest of what SQLite actually holds.
pub(crate) fn hash_tables(conn: &Connection) -> Result<String, JournalError> {
    let mut rows = Vec::new();
    for spec in TABLES {
        let mut statement = conn.prepare(&format!(
            "SELECT {} FROM {}",
            spec.columns.join(", "),
            spec.name
        ))?;
        let mut cursor = statement.query([])?;
        while let Some(row) = cursor.next()? {
            let mut values = Vec::with_capacity(spec.columns.len());
            for index in 0..spec.columns.len() {
                values.push(row.get::<_, Sql>(index)?);
            }
            rows.push((spec.name, values));
        }
    }
    Ok(digest(rows.into_iter()))
}
