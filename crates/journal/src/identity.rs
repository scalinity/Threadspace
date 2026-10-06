//! Provider identity, activations, surface bindings and route results
//! (SPEC §4.1, §4.2, §4.6, §13.2) for the M0B direct-Claude path.
//!
//! One discovery application is one registration transaction: its
//! observation, Session upserts, activation ends/starts and binding proofs
//! commit together or not at all, and nothing is written when nothing changed.
//! Activations end by ID only, so an old end cannot close a newer activation;
//! bindings are invalidated, never deleted, and a binding can only be created
//! for a live activation whose ProcessKey it carries.

use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};
use threadspace_contracts::route::RouteResult;
use uuid::Uuid;

use crate::{Change, Journal, JournalError, enum_text};

pub const SOURCE_CLAUDE_INVENTORY: &str = "claude.inventory";
pub const SOURCE_ROUTE: &str = "companion.route";
pub const SURFACE_TERMINAL: &str = "terminal.app";
pub const PROOF_NATIVE_INVENTORY: &str = "NATIVE_INVENTORY";

/// A process incarnation (ProcessKey on this endpoint) and its executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRecord {
    pub boot_id: String,
    pub pid: u32,
    pub start_seconds: u64,
    pub start_microseconds: u32,
    pub executable: String,
}

/// A Terminal surface proven for an activation: exactly one live tab whose
/// TTY's `st_rdev` equals the provider's `e_tdev`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceRecord {
    pub tty: String,
    pub device: u32,
    pub terminal_generation: String,
    pub window_hint: i64,
    pub tab_hint: i64,
    pub proof: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedSessionRecord {
    pub native_session_id: String,
    pub kind: Option<String>,
    pub display_name: Option<String>,
    pub status: Option<String>,
    pub waiting_for: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationChange {
    /// End one stored activation by ID and invalidate its bindings.
    End {
        execution_id: String,
        reason: String,
    },
    /// A new activation proven by a bracketed join, with its surface if one
    /// was proven (`Err` carries why not).
    Start {
        native_session_id: String,
        process: ProcessRecord,
        device: u32,
        surface: Result<SurfaceRecord, String>,
    },
    /// A surface outcome for an existing live activation without a binding.
    Surface {
        execution_id: String,
        surface: Result<SurfaceRecord, String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryApplication {
    pub provider: String,
    pub profile_ref: String,
    /// Every full session identity in the latest inventory response.
    pub sessions: Vec<ObservedSessionRecord>,
    pub changes: Vec<ActivationChange>,
    /// Sanitized pass summary journaled with the observation.
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApplyOutcome {
    pub change: Option<Change>,
    pub executions_started: u32,
    pub executions_ended: u32,
    pub bindings_recorded: u32,
    pub bindings_invalidated: u32,
}

/// A stored live activation, for reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveExecutionRow {
    pub execution_id: String,
    pub session_id: String,
    pub native_session_id: String,
    pub pid: u32,
    pub start_seconds: u64,
    pub start_microseconds: u32,
    pub executable: String,
    pub has_valid_binding: bool,
}

/// A valid binding of a live activation, with everything a route revalidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingRow {
    pub binding_id: String,
    pub revision: i64,
    pub execution_id: String,
    pub endpoint_id: String,
    pub boot_id: String,
    pub pid: u32,
    pub start_seconds: u64,
    pub start_microseconds: u32,
    pub executable: String,
    pub device: u32,
    pub tty: String,
    pub terminal_generation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteTargetRow {
    NotFound,
    Fixture,
    Unbound {
        native_session_id: String,
        reason: String,
    },
    Bound {
        native_session_id: String,
        bindings: Vec<BindingRow>,
    },
}

fn namespace_id(
    tx: &Transaction<'_>,
    provider: &str,
    endpoint_id: &str,
    profile_ref: &str,
) -> Result<String, JournalError> {
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM provider_namespaces WHERE provider = ?1 AND endpoint_id = ?2 AND profile_ref = ?3",
            params![provider, endpoint_id, profile_ref],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO provider_namespaces (id, provider, endpoint_id, profile_ref) VALUES (?1, ?2, ?3, ?4)",
        params![id, provider, endpoint_id, profile_ref],
    )?;
    Ok(id)
}

fn display_name(record: &ObservedSessionRecord) -> String {
    record.display_name.clone().unwrap_or_else(|| {
        let short: String = record.native_session_id.chars().take(8).collect();
        format!("claude {short}")
    })
}

/// Stored session fields an inventory row can change: (id, kind, status,
/// waiting-for, display name, inventory presence).
type StoredSession = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    i64,
);

/// The session row for a native ID in a namespace: (id, changed-or-new).
fn upsert_session(
    tx: &Transaction<'_>,
    namespace: &str,
    record: &ObservedSessionRecord,
    cursor: i64,
) -> Result<(String, bool), JournalError> {
    let current: Option<StoredSession> = tx
        .query_row(
            "SELECT id, provider_kind, provider_status, provider_waiting_for, display_name, inventory_present
               FROM sessions WHERE namespace_id = ?1 AND native_session_id = ?2",
            params![namespace, record.native_session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        )
        .optional()?;
    let name = display_name(record);
    match current {
        None => {
            let id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO sessions (id, namespace_id, native_session_id, record_state, display_name, fixture,
                   revision, provider_kind, provider_status, provider_waiting_for, inventory_present)
                 VALUES (?1, ?2, ?3, 'KNOWN', ?4, 0, ?5, ?6, ?7, ?8, 1)",
                params![
                    id,
                    namespace,
                    record.native_session_id,
                    name,
                    cursor,
                    record.kind,
                    record.status,
                    record.waiting_for
                ],
            )?;
            Ok((id, true))
        }
        Some((id, kind, status, waiting, old_name, present)) => {
            let changed = kind != record.kind
                || status != record.status
                || waiting != record.waiting_for
                || old_name != name
                || present != 1;
            if changed {
                tx.execute(
                    "UPDATE sessions SET provider_kind = ?2, provider_status = ?3, provider_waiting_for = ?4,
                       display_name = ?5, inventory_present = 1, revision = ?6 WHERE id = ?1",
                    params![id, record.kind, record.status, record.waiting_for, name, cursor],
                )?;
            }
            Ok((id, changed))
        }
    }
}

fn process_id(
    tx: &Transaction<'_>,
    endpoint_id: &str,
    process: &ProcessRecord,
) -> Result<String, JournalError> {
    let existing: Option<(String, String)> = tx
        .query_row(
            "SELECT id, executable_identity FROM process_incarnations
              WHERE endpoint_id = ?1 AND boot_id = ?2 AND pid = ?3 AND start_seconds = ?4 AND start_microseconds = ?5",
            params![
                endpoint_id,
                process.boot_id,
                process.pid,
                process.start_seconds as i64,
                process.start_microseconds
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((id, executable)) = existing {
        if executable != process.executable {
            // Same ProcessKey, new image: the latest qualified image is
            // recorded; bindings keep the image they were proven with.
            tx.execute(
                "UPDATE process_incarnations SET executable_identity = ?2 WHERE id = ?1",
                params![id, process.executable],
            )?;
        }
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO process_incarnations (id, endpoint_id, boot_id, pid, start_seconds, start_microseconds, executable_identity)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            id,
            endpoint_id,
            process.boot_id,
            process.pid,
            process.start_seconds as i64,
            process.start_microseconds,
            process.executable
        ],
    )?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
fn insert_binding(
    tx: &Transaction<'_>,
    session_id: &str,
    execution_id: &str,
    process_id: &str,
    executable: &str,
    surface: &SurfaceRecord,
    observation_id: &str,
    cursor: i64,
) -> Result<(), JournalError> {
    tx.execute(
        "INSERT INTO surface_bindings (id, session_id, execution_id, surface_kind, native_locator, proof, revision,
           valid, process_id, executable_identity, device_number, terminal_generation, window_hint, tab_hint,
           evidence_observation, proof_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            Uuid::new_v4().to_string(),
            session_id,
            execution_id,
            SURFACE_TERMINAL,
            surface.tty,
            PROOF_NATIVE_INVENTORY,
            cursor,
            process_id,
            executable,
            surface.device,
            surface.terminal_generation,
            surface.window_hint,
            surface.tab_hint,
            observation_id,
            surface.proof.to_string()
        ],
    )?;
    tx.execute(
        "UPDATE executions SET surface_status = 'BOUND' WHERE id = ?1",
        params![execution_id],
    )?;
    Ok(())
}

impl Journal {
    /// Stored live activations of one provider, for reconciliation.
    pub fn live_executions(&self, provider: &str) -> Result<Vec<LiveExecutionRow>, JournalError> {
        let mut statement = self.conn.prepare(
            "SELECT e.id, s.id, s.native_session_id, p.pid, p.start_seconds, p.start_microseconds,
                    p.executable_identity,
                    EXISTS (SELECT 1 FROM surface_bindings b WHERE b.execution_id = e.id AND b.valid = 1)
               FROM executions e
               JOIN sessions s ON s.id = e.session_id
               JOIN provider_namespaces n ON n.id = s.namespace_id
               JOIN process_incarnations p ON p.id = e.process_id
              WHERE e.presence = 'LIVE' AND n.provider = ?1 AND p.endpoint_id = ?2
              ORDER BY e.rowid",
        )?;
        let rows = statement
            .query_map(params![provider, self.endpoint_id], |row| {
                Ok(LiveExecutionRow {
                    execution_id: row.get(0)?,
                    session_id: row.get(1)?,
                    native_session_id: row.get(2)?,
                    pid: row.get(3)?,
                    start_seconds: row.get::<_, i64>(4)? as u64,
                    start_microseconds: row.get(5)?,
                    executable: row.get(6)?,
                    has_valid_binding: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Commits one discovery pass as a single registration transaction, or
    /// nothing when the pass changed nothing.
    pub fn apply_discovery(
        &mut self,
        application: &DiscoveryApplication,
        now_ms: i64,
    ) -> Result<ApplyOutcome, JournalError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut outcome = ApplyOutcome::default();
        let observation_id = Uuid::new_v4().to_string();
        // The observation row comes first so bindings can reference it; it is
        // rolled back with everything else if nothing changed.
        let (observation_id, cursor) = {
            tx.execute(
                "INSERT INTO observations (observation_id, source_id, source_epoch, native_event,
                   captured_wall_ms, received_wall_ms, payload_version, payload_json)
                 VALUES (?1, ?2, ?3, 'PROVIDER_SNAPSHOT_OBSERVED', ?4, ?4, 1, ?5)",
                params![
                    observation_id,
                    SOURCE_CLAUDE_INVENTORY,
                    self.source_epoch,
                    now_ms,
                    application.payload.to_string()
                ],
            )?;
            (observation_id, tx.last_insert_rowid())
        };
        let namespace = namespace_id(
            &tx,
            &application.provider,
            &self.endpoint_id,
            &application.profile_ref,
        )?;
        let mut touched: Vec<String> = Vec::new();
        let touch = |touched: &mut Vec<String>, id: &str| {
            if !touched.iter().any(|t| t == id) {
                touched.push(id.to_owned());
            }
        };

        let mut present = Vec::new();
        for record in &application.sessions {
            let (id, changed) = upsert_session(&tx, &namespace, record, cursor)?;
            if changed {
                touch(&mut touched, &id);
            }
            present.push(id);
        }
        // Sessions that left the inventory stay; only their presence flag
        // changes. Absence is not deletion (SPEC §4.15).
        {
            let mut statement = tx.prepare(
                "SELECT id FROM sessions WHERE namespace_id = ?1 AND inventory_present = 1",
            )?;
            let previously: Vec<String> = statement
                .query_map(params![namespace], |row| row.get(0))?
                .collect::<Result<_, _>>()?;
            drop(statement);
            for id in previously.into_iter().filter(|id| !present.contains(id)) {
                tx.execute(
                    "UPDATE sessions SET inventory_present = 0, revision = ?2 WHERE id = ?1",
                    params![id, cursor],
                )?;
                touch(&mut touched, &id);
            }
        }

        for change in &application.changes {
            match change {
                ActivationChange::End {
                    execution_id,
                    reason,
                } => {
                    let session: Option<String> = tx
                        .query_row(
                            "SELECT session_id FROM executions WHERE id = ?1 AND presence = 'LIVE'",
                            params![execution_id],
                            |row| row.get(0),
                        )
                        .optional()?;
                    let Some(session) = session else { continue };
                    tx.execute(
                        "UPDATE executions SET presence = 'ENDED', end_reason = ?2, ended_cursor = ?3
                          WHERE id = ?1 AND presence = 'LIVE'",
                        params![execution_id, reason, cursor],
                    )?;
                    outcome.executions_ended += 1;
                    outcome.bindings_invalidated += tx.execute(
                        "UPDATE surface_bindings SET valid = 0, invalidated_reason = ?2, invalidated_cursor = ?3
                          WHERE execution_id = ?1 AND valid = 1",
                        params![execution_id, reason, cursor],
                    )? as u32;
                    touch(&mut touched, &session);
                }
                ActivationChange::Start {
                    native_session_id,
                    process,
                    device,
                    surface,
                } => {
                    let session: Option<String> = tx
                        .query_row(
                            "SELECT id FROM sessions WHERE namespace_id = ?1 AND native_session_id = ?2",
                            params![namespace, native_session_id],
                            |row| row.get(0),
                        )
                        .optional()?;
                    let session = match session {
                        Some(id) => id,
                        None => {
                            let record = ObservedSessionRecord {
                                native_session_id: native_session_id.clone(),
                                kind: Some("interactive".into()),
                                display_name: None,
                                status: None,
                                waiting_for: None,
                            };
                            upsert_session(&tx, &namespace, &record, cursor)?.0
                        }
                    };
                    let process_id = process_id(&tx, &self.endpoint_id, process)?;
                    // Idempotent: a live activation of this session in this
                    // incarnation is not started twice.
                    let live: Option<String> = tx
                        .query_row(
                            "SELECT id FROM executions WHERE session_id = ?1 AND process_id = ?2 AND presence = 'LIVE'",
                            params![session, process_id],
                            |row| row.get(0),
                        )
                        .optional()?;
                    if live.is_some() {
                        continue;
                    }
                    let activation: i64 = tx.query_row(
                        "SELECT COALESCE(MAX(activation), 0) + 1 FROM executions WHERE session_id = ?1",
                        params![session],
                        |row| row.get(0),
                    )?;
                    let execution_id = Uuid::new_v4().to_string();
                    tx.execute(
                        "INSERT INTO executions (id, session_id, activation, mode, presence, process_id,
                           device_number, started_cursor, surface_status)
                         VALUES (?1, ?2, ?3, 'terminal_embedded', 'LIVE', ?4, ?5, ?6, ?7)",
                        params![
                            execution_id,
                            session,
                            activation,
                            process_id,
                            device,
                            cursor,
                            surface.as_ref().err()
                        ],
                    )?;
                    tx.execute(
                        "INSERT INTO execution_processes (execution_id, process_id, role) VALUES (?1, ?2, 'provider')",
                        params![execution_id, process_id],
                    )?;
                    outcome.executions_started += 1;
                    if let Ok(surface) = surface {
                        insert_binding(
                            &tx,
                            &session,
                            &execution_id,
                            &process_id,
                            &process.executable,
                            surface,
                            &observation_id,
                            cursor,
                        )?;
                        outcome.bindings_recorded += 1;
                    }
                    touch(&mut touched, &session);
                }
                ActivationChange::Surface {
                    execution_id,
                    surface,
                } => {
                    let live: Option<(String, String, String, Option<String>)> = tx
                        .query_row(
                            "SELECT e.session_id, e.process_id, p.executable_identity, e.surface_status
                               FROM executions e JOIN process_incarnations p ON p.id = e.process_id
                              WHERE e.id = ?1 AND e.presence = 'LIVE'
                                AND NOT EXISTS (SELECT 1 FROM surface_bindings b
                                                 WHERE b.execution_id = e.id AND b.valid = 1)",
                            params![execution_id],
                            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                        )
                        .optional()?;
                    let Some((session, process_id, executable, status)) = live else {
                        continue;
                    };
                    match surface {
                        Ok(surface) => {
                            insert_binding(
                                &tx,
                                &session,
                                execution_id,
                                &process_id,
                                &executable,
                                surface,
                                &observation_id,
                                cursor,
                            )?;
                            outcome.bindings_recorded += 1;
                            touch(&mut touched, &session);
                        }
                        Err(reason) if status.as_deref() != Some(reason.as_str()) => {
                            tx.execute(
                                "UPDATE executions SET surface_status = ?2 WHERE id = ?1",
                                params![execution_id, reason],
                            )?;
                            touch(&mut touched, &session);
                        }
                        Err(_) => {}
                    }
                }
            }
        }

        if touched.is_empty() {
            tx.rollback()?;
            return Ok(outcome);
        }
        for id in &touched {
            tx.execute(
                "UPDATE sessions SET revision = ?2 WHERE id = ?1",
                params![id, cursor],
            )?;
        }
        tx.commit()?;
        outcome.change = Some(Change {
            cursor,
            session_ids: touched,
            attention_ids: Vec::new(),
        });
        Ok(outcome)
    }

    /// Everything a route needs about a session, read in one transaction.
    pub fn route_target(&mut self, session_id: &str) -> Result<RouteTargetRow, JournalError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let session: Option<(String, bool, Option<String>)> = tx
            .query_row(
                "SELECT native_session_id, fixture, provider_kind FROM sessions WHERE id = ?1",
                params![session_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((native_session_id, fixture, kind)) = session else {
            return Ok(RouteTargetRow::NotFound);
        };
        if fixture {
            return Ok(RouteTargetRow::Fixture);
        }
        let mut statement = tx.prepare(
            "SELECT b.id, b.revision, b.execution_id, p.endpoint_id, p.boot_id, p.pid, p.start_seconds,
                    p.start_microseconds, b.executable_identity, b.device_number, b.native_locator,
                    b.terminal_generation
               FROM surface_bindings b
               JOIN executions e ON e.id = b.execution_id
               JOIN process_incarnations p ON p.id = b.process_id
              WHERE b.session_id = ?1 AND b.valid = 1 AND e.presence = 'LIVE'
              ORDER BY e.activation",
        )?;
        let bindings = statement
            .query_map(params![session_id], |row| {
                Ok(BindingRow {
                    binding_id: row.get(0)?,
                    revision: row.get(1)?,
                    execution_id: row.get(2)?,
                    endpoint_id: row.get(3)?,
                    boot_id: row.get(4)?,
                    pid: row.get(5)?,
                    start_seconds: row.get::<_, i64>(6)? as u64,
                    start_microseconds: row.get(7)?,
                    executable: row.get(8)?,
                    device: row.get(9)?,
                    tty: row.get(10)?,
                    terminal_generation: row.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        if !bindings.is_empty() {
            return Ok(RouteTargetRow::Bound {
                native_session_id,
                bindings,
            });
        }
        let live_status: Option<Option<String>> = tx
            .query_row(
                "SELECT surface_status FROM executions WHERE session_id = ?1 AND presence = 'LIVE'
                  ORDER BY activation DESC LIMIT 1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()?;
        let reason = match (live_status, kind.as_deref()) {
            (Some(status), _) => status.unwrap_or_else(|| "SURFACE_UNPROVEN".into()),
            (None, Some(kind)) if kind != "interactive" => "UNSUPPORTED_KIND".into(),
            (None, _) => "NO_LIVE_MAPPING".into(),
        };
        Ok(RouteTargetRow::Unbound {
            native_session_id,
            reason,
        })
    }

    /// A binding's revision if it is valid and its activation is live.
    pub fn binding_revision(&self, binding_id: &str) -> Result<Option<i64>, JournalError> {
        Ok(self
            .conn
            .query_row(
                "SELECT b.revision FROM surface_bindings b JOIN executions e ON e.id = b.execution_id
                  WHERE b.id = ?1 AND b.valid = 1 AND e.presence = 'LIVE'",
                params![binding_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Journals one Return attempt and its evidence. Recording is separate
    /// from any attention acknowledgement (SPEC §18.3).
    pub fn record_route(
        &mut self,
        result: &RouteResult,
        now_ms: i64,
    ) -> Result<Change, JournalError> {
        let evidence =
            serde_json::to_string(&result.evidence).map_err(|error| JournalError::Invalid {
                detail: error.to_string(),
            })?;
        let payload = json!({
            "requestId": result.request_id,
            "sessionId": result.session_id,
            "surfaceResult": enum_text(&result.surface_result),
            "sessionVerification": enum_text(&result.session_verification),
            "inputReadiness": enum_text(&result.input_readiness),
            "reasonCode": result.reason_code,
            "focusPerformed": result.focus_performed,
            "latencyMs": result.latency_ms,
        });
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM sessions WHERE id = ?1)",
            params![result.session_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(JournalError::NotFound {
                entity: "session",
                id: result.session_id.clone(),
            });
        }
        let observation_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO observations (observation_id, source_id, source_epoch, native_event,
               captured_wall_ms, received_wall_ms, payload_version, payload_json)
             VALUES (?1, ?2, ?3, 'ROUTE_RESULT_RECORDED', ?4, ?5, 1, ?6)",
            params![
                observation_id,
                SOURCE_ROUTE,
                self.source_epoch,
                result.started_at_ms,
                now_ms,
                payload.to_string()
            ],
        )?;
        let cursor = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO route_results (request_id, session_id, binding_id, binding_revision, surface_result,
               session_verification, input_readiness, reason_code, focus_performed, latency_ms, started_at_ms,
               recorded_at_ms, observation_id, evidence_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                result.request_id,
                result.session_id,
                result.binding_id,
                result
                    .evidence
                    .binding_revision_loaded
                    .as_deref()
                    .and_then(|r| r.parse::<i64>().ok()),
                enum_text(&result.surface_result),
                enum_text(&result.session_verification),
                enum_text(&result.input_readiness),
                result.reason_code,
                result.focus_performed,
                result.latency_ms,
                result.started_at_ms,
                now_ms,
                observation_id,
                evidence
            ],
        )?;
        tx.execute(
            "UPDATE sessions SET revision = ?2 WHERE id = ?1",
            params![result.session_id, cursor],
        )?;
        tx.commit()?;
        Ok(Change {
            cursor,
            session_ids: vec![result.session_id.clone()],
            attention_ids: Vec::new(),
        })
    }

    /// A recorded route's evidence (for diagnostics and evidence export).
    pub fn route_evidence(&self, request_id: &str) -> Result<Option<String>, JournalError> {
        Ok(self
            .conn
            .query_row(
                "SELECT evidence_json FROM route_results WHERE request_id = ?1",
                params![request_id],
                |row| row.get(0),
            )
            .optional()?)
    }
}
