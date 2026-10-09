//! Reads provider-neutral view models from the committed store. M0A uses this
//! bounded fixture projection; M1 replaces it with the canonical reducer's
//! materialized projections.

use rusqlite::{Connection, OptionalExtension, params};
use serde::de::DeserializeOwned;
use threadspace_contracts::cursor::format_cursor;
use threadspace_contracts::projection::{
    AttentionCounts, AttentionView, BindingView, ExecutionPresence, ObservationState, ProcessView,
    SessionView, TurnState,
};
use threadspace_contracts::route::RouteSummary;

use crate::JournalError;

fn parse_enum<T: DeserializeOwned>(text: String) -> Result<T, JournalError> {
    serde_json::from_value(serde_json::Value::String(text.clone())).map_err(|_| {
        JournalError::Invalid {
            detail: format!("unknown stored state {text}"),
        }
    })
}

type SessionRow = (
    String,
    String,
    String,
    bool,
    i64,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
);

/// The canonical sessions a view reads its observer's evidence tier and
/// version from (D-0010): the writer's engine state, which is the state the
/// rows were committed from.
pub type Observers = std::collections::BTreeMap<String, threadspace_contracts::canonical::records::SessionRecord>;

pub fn session(conn: &Connection, session_id: &str, observers: &Observers) -> Result<SessionView, JournalError> {
    let row: Option<SessionRow> = conn
        .query_row(
            "SELECT n.provider, s.native_session_id, s.display_name, s.fixture, s.revision,
                    s.provider_status, s.provider_waiting_for,
                    s.turn_state, s.execution_presence, s.observation
               FROM sessions s JOIN provider_namespaces n ON n.id = s.namespace_id
              WHERE s.id = ?1",
            params![session_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ))
            },
        )
        .optional()?;
    let Some((
        provider,
        native_session_id,
        display_name,
        fixture,
        revision,
        provider_status,
        provider_waiting_for,
        turn_state,
        execution_presence,
        observation,
    )) = row
    else {
        return Err(JournalError::NotFound {
            entity: "session",
            id: session_id.to_owned(),
        });
    };

    // Lifecycle state is the canonical session's, as materialized (D-0007 §6):
    // never re-derived here from activation or turn arrival order.
    let turn_state: TurnState = parse_enum(turn_state)?;
    let execution_presence: ExecutionPresence = parse_enum(execution_presence)?;
    let observation: ObservationState = parse_enum(observation)?;
    let observer = observers.get(session_id);
    let observer_tier = observer.and_then(|s| s.observer_tier);
    let observer_version = observer.and_then(|s| s.observer_version.clone());
    let link_conflict = observer.is_some_and(|s| s.link_conflict);

    // The displayed activation (its process and binding): the newest live
    // one, else the newest.
    let execution: Option<(String, i64)> = conn
        .query_row(
            "SELECT id, activation FROM executions WHERE session_id = ?1
              ORDER BY presence = 'LIVE' DESC, activation DESC LIMIT 1",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    let mut activation = None;
    let mut process = None;
    let mut binding = None;
    if let Some((execution_id, activation_value)) = execution {
        activation = Some(activation_value.to_string());
        process = conn
            .query_row(
                "SELECT p.pid, p.boot_id, p.start_seconds, p.start_microseconds, p.executable_identity
                   FROM execution_processes ep JOIN process_incarnations p ON p.id = ep.process_id
                  WHERE ep.execution_id = ?1 ORDER BY p.start_seconds DESC LIMIT 1",
                params![execution_id],
                |row| {
                    Ok(ProcessView {
                        pid: row.get(0)?,
                        boot_id: row.get(1)?,
                        start_seconds: row.get::<_, i64>(2)?.to_string(),
                        start_microseconds: row.get(3)?,
                        executable_identity: row.get(4)?,
                    })
                },
            )
            .optional()?;
        binding = conn
            .query_row(
                "SELECT b.id, b.surface_kind, b.proof, b.revision, b.native_locator, b.device_number, p.pid
                   FROM surface_bindings b LEFT JOIN process_incarnations p ON p.id = b.process_id
                  WHERE b.execution_id = ?1 AND b.valid = 1 ORDER BY b.revision DESC LIMIT 1",
                params![execution_id],
                |row| {
                    Ok(BindingView {
                        binding_id: row.get(0)?,
                        surface_kind: row.get(1)?,
                        proof: row.get(2)?,
                        revision: format_cursor(row.get(3)?),
                        locator: row.get(4)?,
                        device_number: row.get(5)?,
                        pid: row.get(6)?,
                    })
                },
            )
            .optional()?;
    }

    let live_bindings: u32 = conn.query_row(
        "SELECT COUNT(*) FROM surface_bindings b JOIN executions e ON e.id = b.execution_id
          WHERE b.session_id = ?1 AND b.valid = 1 AND e.presence = 'LIVE'",
        params![session_id],
        |row| row.get(0),
    )?;
    let last_invalidation: Option<String> = conn
        .query_row(
            "SELECT invalidated_reason FROM surface_bindings
              WHERE session_id = ?1 AND valid = 0 AND invalidated_reason IS NOT NULL
              ORDER BY invalidated_cursor DESC LIMIT 1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()?;
    let last_route = conn
        .query_row(
            "SELECT request_id, surface_result, session_verification, input_readiness, reason_code,
                    focus_performed, latency_ms, recorded_at_ms
               FROM route_results WHERE session_id = ?1 ORDER BY recorded_at_ms DESC, rowid DESC LIMIT 1",
            params![session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, bool>(5)?,
                    row.get::<_, u32>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )
        .optional()?;
    let last_route = match last_route {
        Some((request_id, surface, verification, readiness, reason, focus, latency, at)) => {
            Some(RouteSummary {
                request_id,
                surface_result: parse_enum(surface)?,
                session_verification: parse_enum(verification)?,
                input_readiness: parse_enum(readiness)?,
                reason_code: reason,
                focus_performed: focus,
                latency_ms: latency,
                recorded_at_ms: at,
            })
        }
        None => None,
    };

    Ok(SessionView {
        session_id: session_id.to_owned(),
        provider,
        native_session_id,
        display_name,
        activation,
        turn_state,
        execution_presence,
        observation,
        observer_tier,
        observer_version,
        link_conflict,
        process,
        binding,
        live_bindings,
        last_invalidation,
        provider_status,
        provider_waiting_for,
        last_route,
        fixture,
        revision: format_cursor(revision),
    })
}

pub fn sessions(conn: &Connection, observers: &Observers) -> Result<Vec<SessionView>, JournalError> {
    let mut statement = conn.prepare("SELECT id FROM sessions ORDER BY revision DESC")?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.iter().map(|id| session(conn, id, observers)).collect()
}

fn attention_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(AttentionView, String, String)> {
    Ok((
        AttentionView {
            attention_id: row.get(0)?,
            session_id: row.get(1)?,
            turn_id: row.get(2)?,
            category: threadspace_contracts::projection::AttentionCategory::TurnComplete,
            priority: row.get(4)?,
            summary: row.get(5)?,
            created_at_ms: row.get(6)?,
            acknowledged_at_ms: row.get(7)?,
            resolved_at_ms: row.get(8)?,
            notification_state: threadspace_contracts::projection::NotificationState::NotRequested,
            revision: format_cursor(row.get(10)?),
        },
        row.get(3)?,
        row.get(9)?,
    ))
}

const ATTENTION_COLUMNS: &str =
    "id, session_id, turn_id, category, priority, summary, created_at_ms,
    acknowledged_at_ms, resolved_at_ms, notification_state, revision";

fn finish_attention(
    (mut view, category, notification_state): (AttentionView, String, String),
) -> Result<AttentionView, JournalError> {
    view.category = parse_enum(category)?;
    view.notification_state = parse_enum(notification_state)?;
    Ok(view)
}

pub fn attention(conn: &Connection, attention_id: &str) -> Result<AttentionView, JournalError> {
    let row = conn
        .query_row(
            &format!("SELECT {ATTENTION_COLUMNS} FROM attention_items WHERE id = ?1"),
            params![attention_id],
            attention_from_row,
        )
        .optional()?
        .ok_or_else(|| JournalError::NotFound {
            entity: "attention",
            id: attention_id.to_owned(),
        })?;
    finish_attention(row)
}

/// Every unresolved item, highest priority first, oldest first within a
/// priority (SPEC §7.4). Grouping never hides an item.
pub fn open_attention(conn: &Connection) -> Result<Vec<AttentionView>, JournalError> {
    let mut statement = conn.prepare(&format!(
        "SELECT {ATTENTION_COLUMNS} FROM attention_items WHERE resolved_at_ms IS NULL
          ORDER BY priority DESC, created_at_ms ASC"
    ))?;
    let rows = statement
        .query_map([], attention_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter().map(finish_attention).collect()
}

pub fn counts(conn: &Connection) -> Result<AttentionCounts, JournalError> {
    Ok(conn.query_row(
        "SELECT
           COALESCE(SUM(CASE WHEN resolved_at_ms IS NULL AND acknowledged_at_ms IS NULL THEN 1 ELSE 0 END), 0),
           COALESCE(SUM(CASE WHEN resolved_at_ms IS NULL AND acknowledged_at_ms IS NOT NULL THEN 1 ELSE 0 END), 0)
         FROM attention_items",
        [],
        |row| Ok(AttentionCounts { needs_attention: row.get(0)?, awaiting_action: row.get(1)? }),
    )?)
}
