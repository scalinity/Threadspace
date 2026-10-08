//! Provider identity, activations, surface bindings and route results
//! (SPEC §4.1, §4.2, §4.6, §13.2) for the M0B direct-Claude path.
//!
//! One discovery application is one registration transaction: its
//! observation, Session upserts, activation ends/starts and binding proofs
//! commit together or not at all, and nothing is written when nothing changed.
//! Activations end by ID only, so an old end cannot close a newer activation;
//! bindings are invalidated, never deleted, and a binding can only be created
//! for a live activation whose ProcessKey it carries.

use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use threadspace_contracts::canonical::fact::{
    AttachedPresence, BindingMethod, BindingProof, CanonicalRefs, Delivery, EvidenceClass,
    ExecutionMode, FactPayload, NativeFactDraft, NativeRefs, SnapshotInterval, SnapshotRow,
};
use threadspace_contracts::canonical::keys::{NativeExecutionRef, NativeSessionRef, NativeSurfaceRef};
use threadspace_contracts::projection::ExecutionPresence;
use threadspace_contracts::route::{ProcessKey, RouteResult};
use threadspace_state_engine::keys;

use crate::{Change, Journal, JournalError, enum_text};

/// A Terminal surface proof (or why none) for an activation.
fn surface_draft(
    refs: NativeRefs,
    generation: &str,
    executable: &str,
    surface: &Result<SurfaceRecord, String>,
) -> NativeFactDraft {
    match surface {
        Ok(surface) => NativeFactDraft {
            refs: NativeRefs {
                surface: Some(NativeSurfaceRef {
                    surface_kind: SURFACE_TERMINAL.into(),
                    app_generation: surface.terminal_generation.clone(),
                    locator: surface.tty.clone(),
                    device_number: Some(surface.device),
                    surface_generation: generation.to_owned(),
                }),
                ..refs
            },
            provenance: EvidenceClass::ProviderSnapshot,
            causal: None,
            payload: FactPayload::SurfaceBindingRecorded {
                proof: BindingProof {
                    method: BindingMethod::NativeInventory,
                    executable_identity: Some(executable.to_owned()),
                    window_hint: Some(surface.window_hint),
                    tab_hint: Some(surface.tab_hint),
                    evidence: surface.proof.clone(),
                },
            },
        },
        Err(reason) => NativeFactDraft {
            refs,
            provenance: EvidenceClass::ProviderSnapshot,
            causal: None,
            payload: FactPayload::SurfaceBindingUnproven {
                reason: reason.clone(),
            },
        },
    }
}

pub const SOURCE_CLAUDE_INVENTORY: &str = "claude.inventory";
pub const SOURCE_ROUTE: &str = "companion.route";
pub const SURFACE_TERMINAL: &str = "terminal.app";

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
    /// nothing when the pass changed nothing. The pass is reconciled against
    /// the canonical state into native-keyed drafts for the changes alone,
    /// then admitted like every other canonical entry: activations end by
    /// ID only, an ended activation is never bound, and a live activation of
    /// a session in one process incarnation is never started twice.
    #[allow(clippy::too_many_lines)]
    pub fn apply_discovery(
        &mut self,
        application: &DiscoveryApplication,
        now_ms: i64,
    ) -> Result<ApplyOutcome, JournalError> {
        let observation_id = self.allocate_id();
        let state = &self.engine.state;
        let namespace = self
            .index
            .get(&keys::namespace(&application.provider, &self.endpoint_id, &application.profile_ref))
            .map(str::to_owned);
        let session_ref = |native: &str| NativeSessionRef {
            provider: application.provider.clone(),
            profile_ref: application.profile_ref.clone(),
            native_session_id: native.to_owned(),
        };
        let session_id_of = |native: &str| {
            namespace
                .as_ref()
                .and_then(|ns| self.index.get(&keys::session(ns, native)))
                .map(str::to_owned)
        };
        let interval = SnapshotInterval { start_ms: now_ms, end_ms: now_ms };
        let snapshot = |native: &str, present: bool, row: SnapshotRow| NativeFactDraft {
            refs: NativeRefs {
                session: Some(session_ref(native)),
                ..NativeRefs::default()
            },
            provenance: EvidenceClass::ProviderSnapshot,
            causal: None,
            payload: FactPayload::ProviderSnapshotObserved { present, row: Some(row), interval },
        };
        let mut drafts = Vec::new();
        let mut outcome = ApplyOutcome::default();
        let mut listed = BTreeSet::new();
        for record in &application.sessions {
            listed.insert(record.native_session_id.clone());
            let row = SnapshotRow {
                kind: record.kind.clone(),
                status: record.status.clone(),
                waiting_for: record.waiting_for.clone(),
                display_name: record.display_name.clone(),
            };
            let unchanged = session_id_of(&record.native_session_id)
                .and_then(|id| state.sessions.get(&id))
                .and_then(|s| s.inventory.as_ref())
                .is_some_and(|i| i.present && i.row.as_ref() == Some(&row));
            if !unchanged {
                drafts.push(snapshot(&record.native_session_id, true, row));
            }
        }
        // Sessions that left the inventory stay; only their presence changes.
        // Absence is not deletion (SPEC §4.15).
        if let Some(ns) = &namespace {
            for session in state.sessions.values().filter(|s| {
                s.namespace_id == *ns && !listed.contains(&s.native_session_id)
            }) {
                if let Some(inventory) = session.inventory.as_ref().filter(|i| i.present) {
                    drafts.push(snapshot(
                        &session.native_session_id,
                        false,
                        inventory.row.clone().unwrap_or(SnapshotRow {
                            kind: None,
                            status: None,
                            waiting_for: None,
                            display_name: None,
                        }),
                    ));
                }
            }
        }
        let mut ending: BTreeSet<String> = BTreeSet::new();
        let live = |id: &str, ending: &BTreeSet<String>| {
            state
                .executions
                .get(id)
                .is_some_and(|e| e.presence == ExecutionPresence::Live && !ending.contains(id))
        };
        let valid_bindings = |id: &str| {
            state
                .bindings
                .values()
                .filter(|b| b.execution_id == id && b.valid)
                .count() as u32
        };
        for change in &application.changes {
            match change {
                ActivationChange::End {
                    execution_id,
                    reason,
                } => {
                    if !live(execution_id, &ending) {
                        continue;
                    }
                    ending.insert(execution_id.clone());
                    outcome.executions_ended += 1;
                    outcome.bindings_invalidated += valid_bindings(execution_id);
                    drafts.push(NativeFactDraft {
                        refs: NativeRefs {
                            execution: Some(NativeExecutionRef::Canonical {
                                execution_id: execution_id.clone(),
                            }),
                            ..NativeRefs::default()
                        },
                        provenance: EvidenceClass::ProviderSnapshot,
                        causal: None,
                        payload: FactPayload::ExecutionEnded {
                            reason: reason.clone(),
                        },
                    });
                }
                ActivationChange::Start {
                    native_session_id,
                    process,
                    device,
                    surface,
                } => {
                    let key = ProcessKey {
                        endpoint_id: self.endpoint_id.clone(),
                        boot_id: process.boot_id.clone(),
                        pid: process.pid,
                        start_seconds: process.start_seconds.to_string(),
                        start_microseconds: process.start_microseconds,
                    };
                    let process_id = self.index.get(&keys::process(&key)).map(str::to_owned);
                    let session_id = session_id_of(native_session_id);
                    let already = session_id.as_ref().zip(process_id.as_ref()).is_some_and(|(s, p)| {
                        state.executions.values().any(|e| {
                            e.session_id == *s
                                && e.process_id.as_deref() == Some(p.as_str())
                                && live(&e.id, &ending)
                        })
                    });
                    if already {
                        continue;
                    }
                    let known = session_id.as_ref().is_some_and(|id| state.sessions.contains_key(id));
                    if !known && listed.insert(native_session_id.clone()) {
                        drafts.push(snapshot(
                            native_session_id,
                            true,
                            SnapshotRow {
                                kind: Some("interactive".into()),
                                status: None,
                                waiting_for: None,
                                display_name: None,
                            },
                        ));
                    }
                    let generation = format!(
                        "{}:{}.{}",
                        process.pid, process.start_seconds, process.start_microseconds
                    );
                    let execution = NativeExecutionRef::Activation {
                        activation_ref: format!("process:{generation}#{observation_id}"),
                    };
                    let session_refs = NativeRefs {
                        session: Some(session_ref(native_session_id)),
                        ..NativeRefs::default()
                    };
                    drafts.push(NativeFactDraft {
                        refs: NativeRefs { process: Some(key.clone()), ..NativeRefs::default() },
                        provenance: EvidenceClass::Kernel,
                        causal: None,
                        payload: FactPayload::ProcessObserved {
                            executable_identity: process.executable.clone(),
                        },
                    });
                    drafts.push(NativeFactDraft {
                        refs: NativeRefs {
                            execution: Some(execution.clone()),
                            process: Some(key),
                            ..session_refs.clone()
                        },
                        provenance: EvidenceClass::ProviderSnapshot,
                        causal: None,
                        payload: FactPayload::ExecutionAttached {
                            mode: ExecutionMode::TerminalEmbedded,
                            presence: AttachedPresence::Live,
                            native_runtime_id: None,
                            controlling_device: Some(*device),
                        },
                    });
                    outcome.executions_started += 1;
                    drafts.push(surface_draft(
                        NativeRefs { execution: Some(execution), ..session_refs },
                        &generation,
                        &process.executable,
                        surface,
                    ));
                    if surface.is_ok() {
                        outcome.bindings_recorded += 1;
                    }
                }
                ActivationChange::Surface {
                    execution_id,
                    surface,
                } => {
                    let Some(execution) = state.executions.get(execution_id) else {
                        continue;
                    };
                    if !live(execution_id, &ending) || valid_bindings(execution_id) > 0 {
                        continue;
                    }
                    if let Err(reason) = surface
                        && execution.surface_status.as_deref() == Some(reason.as_str())
                    {
                        continue;
                    }
                    let process = execution.process_id.as_ref().and_then(|p| state.processes.get(p));
                    let generation = process.map_or_else(String::new, |p| {
                        format!("{}:{}.{}", p.key.pid, p.key.start_seconds, p.key.start_microseconds)
                    });
                    let executable = process
                        .and_then(|p| p.current_executable.clone())
                        .unwrap_or_default();
                    drafts.push(surface_draft(
                        NativeRefs {
                            execution: Some(NativeExecutionRef::Canonical {
                                execution_id: execution_id.clone(),
                            }),
                            ..NativeRefs::default()
                        },
                        &generation,
                        &executable,
                        surface,
                    ));
                    if surface.is_ok() {
                        outcome.bindings_recorded += 1;
                    }
                }
            }
        }
        if drafts.is_empty() {
            return Ok(outcome);
        }
        let (cursor, output) = self.admit_internal(
            observation_id,
            SOURCE_CLAUDE_INVENTORY,
            "PROVIDER_SNAPSHOT_OBSERVED",
            &application.payload,
            &drafts,
            Vec::new(),
            Delivery::Live,
            now_ms,
            now_ms,
            |_, _, _, _| Ok(()),
        )?;
        outcome.change = Some(Change {
            cursor,
            session_ids: output.changed.sessions.into_iter().collect(),
            attention_ids: output.changed.attention.into_iter().collect(),
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
        if !self.engine.state.sessions.contains_key(&result.session_id) {
            return Err(JournalError::NotFound {
                entity: "session",
                id: result.session_id.clone(),
            });
        }
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
        let observation_id = self.allocate_id();
        let fact = self.prebuilt_fact(
            &observation_id,
            CanonicalRefs {
                session_id: Some(result.session_id.clone()),
                ..CanonicalRefs::default()
            },
            EvidenceClass::Derived,
            FactPayload::RouteResultRecorded {
                request_id: result.request_id.clone(),
                surface_result: result.surface_result,
                session_verification: result.session_verification,
                input_readiness: result.input_readiness,
                reason_code: result.reason_code.chars().take(64).collect(),
                focus_performed: result.focus_performed,
            },
        );
        let (cursor, _) = self.admit_internal(
            observation_id,
            SOURCE_ROUTE,
            "ROUTE_RESULT_RECORDED",
            &payload,
            &[],
            vec![fact],
            Delivery::Live,
            result.started_at_ms,
            now_ms,
            |tx, observation_id, _, _| {
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
                Ok(())
            },
        )?;
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

/// One journaled identity/route observation, exported for qualification
/// evidence. Payloads are the sanitized records the companion wrote.
#[cfg(feature = "qualification")]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationExport {
    pub cursor: i64,
    pub observation_id: String,
    pub source_id: String,
    pub native_event: String,
    pub captured_wall_ms: i64,
    pub received_wall_ms: i64,
    pub payload: Value,
    /// For a route result, its full evidence chain.
    pub route_evidence: Option<Value>,
}

#[cfg(feature = "qualification")]
impl Journal {
    /// Identity and route observations after `after_cursor`, oldest first.
    pub fn export_observations(
        &self,
        after_cursor: i64,
        limit: u32,
    ) -> Result<Vec<ObservationExport>, JournalError> {
        let mut statement = self.conn.prepare(
            "SELECT o.ingest_seq, o.observation_id, o.source_id, o.native_event, o.captured_wall_ms,
                    o.received_wall_ms, o.payload_json, r.evidence_json
               FROM observations o LEFT JOIN route_results r ON r.observation_id = o.observation_id
              WHERE o.ingest_seq > ?1 AND o.source_id IN (?2, ?3)
              ORDER BY o.ingest_seq LIMIT ?4",
        )?;
        let rows = statement
            .query_map(
                params![
                    after_cursor,
                    SOURCE_CLAUDE_INVENTORY,
                    SOURCE_ROUTE,
                    limit.min(200)
                ],
                |row| {
                    let payload: String = row.get(6)?;
                    let evidence: Option<String> = row.get(7)?;
                    Ok(ObservationExport {
                        cursor: row.get(0)?,
                        observation_id: row.get(1)?,
                        source_id: row.get(2)?,
                        native_event: row.get(3)?,
                        captured_wall_ms: row.get(4)?,
                        received_wall_ms: row.get(5)?,
                        payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
                        route_evidence: evidence.and_then(|text| serde_json::from_str(&text).ok()),
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}
