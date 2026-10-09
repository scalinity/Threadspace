//! The canonical baseline of a store written before M1 (SPEC §9.3: a new
//! reducer migrates the retained representation; it cannot assume raw M0
//! observations can be re-normalized). M0 wrote projection rows directly, so
//! those rows are the retained evidence: each becomes the canonical record it
//! represents, keeping its M0 ID. Records M0 lacked get name-based IDs
//! derived from M0 IDs, so the same old store always upgrades to the same
//! state (deterministic upgrade).

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;
use threadspace_contracts::canonical::fact::{
    AttachedPresence, BindingMethod, ExecutionMode, SessionRecordState, SnapshotInterval,
    SnapshotRow, TurnOutcome,
};
use threadspace_contracts::canonical::keys::{NativeActorRef, NativeSurfaceRef};
use threadspace_contracts::canonical::records::{
    ActorRecord, AttentionRecord, AttentionScope, CanonicalState, CommandEffect, ExecutionRecord,
    InventoryObservation, NamespaceRecord, OutboxRecord, OutboxState, OwnerActionKind,
    ProcessImage, ProcessRecord, ResolutionCause, ResolutionKind, RouteRecord, SessionRecord,
    SourceSurfaceRecord, SummaryAuthority, SurfaceBindingRecord, TurnIdentityKind, TurnRecord,
};
use threadspace_contracts::projection::{
    AttentionCategory, ExecutionPresence, NotificationState, ObservationState, TurnState,
};
use threadspace_contracts::route::ProcessKey;
use threadspace_contracts::canonical::command::{OwnerAction, OwnerCommand};
use threadspace_state_engine::REDUCER_VERSION;
use threadspace_state_engine::hash::sha256_hex;
use threadspace_state_engine::engine::Engine;
use threadspace_state_engine::ids::derived_id;
use threadspace_state_engine::keys;
use threadspace_state_engine::resolve::{Assignment, Entity};

use crate::JournalError;

fn parse<T: serde::de::DeserializeOwned>(text: &str) -> Option<T> {
    serde_json::from_value(Value::String(text.to_owned())).ok()
}

/// Whether the store holds M0 projection rows and no canonical state yet.
pub(crate) fn needed(conn: &Connection) -> Result<bool, JournalError> {
    let checkpoints: i64 =
        conn.query_row("SELECT COUNT(*) FROM projection_checkpoints", [], |row| row.get(0))?;
    let sessions: i64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;
    Ok(checkpoints == 0 && sessions > 0)
}

fn assign(out: &mut Vec<Assignment>, seen: &mut BTreeSet<String>, entity: Entity, key: String, id: &str) {
    if seen.insert(key.clone()) {
        out.push(Assignment {
            native_key: key,
            entity,
            id: id.to_owned(),
        });
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn from_m0(conn: &Connection) -> Result<(CanonicalState, Vec<Assignment>), JournalError> {
    let mut state = CanonicalState {
        reducer_version: REDUCER_VERSION,
        ..CanonicalState::default()
    };
    let mut assignments = Vec::new();
    let mut seen = BTreeSet::new();
    let endpoint: String = conn
        .query_row("SELECT value FROM store_meta WHERE key = 'endpoint_id'", [], |row| row.get(0))
        .optional()?
        .unwrap_or_default();
    let through: i64 =
        conn.query_row("SELECT COALESCE(MAX(ingest_seq), 0) FROM observations", [], |row| row.get(0))?;
    state.through_cursor = through;

    let mut statement = conn.prepare("SELECT id, provider, endpoint_id, profile_ref FROM provider_namespaces")?;
    let rows = statement
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    for (id, provider, endpoint_id, profile_ref) in rows {
        assign(&mut assignments, &mut seen, Entity::Namespace, keys::namespace(&provider, &endpoint_id, &profile_ref), &id);
        state.namespaces.insert(id.clone(), NamespaceRecord { id, provider, endpoint_id, profile_ref });
    }

    let mut statement = conn.prepare(
        "SELECT id, namespace_id, native_session_id, record_state, display_name, fixture, revision,
                provider_kind, provider_status, provider_waiting_for, inventory_present FROM sessions",
    )?;
    type SessionRow = (String, String, String, String, String, bool, i64, Option<String>, Option<String>, Option<String>, i64);
    let rows: Vec<SessionRow> = statement
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?))
        })?
        .collect::<Result<_, _>>()?;
    let mut principal = BTreeMap::new();
    for (id, namespace_id, native, record_state, display, fixture, revision, kind, status, waiting, present) in rows {
        assign(&mut assignments, &mut seen, Entity::Session, keys::session(&namespace_id, &native), &id);
        let actor_id = derived_id(&format!("m0-baseline-actor|{id}"));
        assign(&mut assignments, &mut seen, Entity::Actor, keys::actor(&id, &NativeActorRef::Principal), &actor_id);
        state.actors.insert(
            actor_id.clone(),
            ActorRecord {
                id: actor_id.clone(),
                session_id: id.clone(),
                native: NativeActorRef::Principal,
                role: threadspace_contracts::canonical::fact::ActorRole::Principal,
                agent_types: BTreeSet::new(),
                runs_ended: 0,
                created_cursor: revision,
                revision,
            },
        );
        principal.insert(id.clone(), actor_id);
        let inventory = (kind.is_some() || present == 1).then_some(InventoryObservation {
            present: present == 1,
            row: Some(SnapshotRow { kind, status, waiting_for: waiting, display_name: None, state: None }),
            interval: SnapshotInterval { start_ms: 0, end_ms: 0 },
            point: None,
        });
        state.sessions.insert(
            id.clone(),
            SessionRecord {
                id,
                namespace_id,
                native_session_id: native,
                record_state: parse(&record_state).unwrap_or(SessionRecordState::Known),
                display_name: Some(display),
                start_sources: BTreeSet::new(),
                links: BTreeSet::new(),
                link: None,
                inventory,
                last_route: None,
                fixture,
                execution_presence: ExecutionPresence::Unknown,
                observation: ObservationState::Unknown,
                turn_state: TurnState::Unknown,
                link_conflict: false,
                observer_tier: None,
                observer_version: None,
                created_cursor: revision,
                revision,
            },
        );
    }

    let mut statement = conn.prepare(
        "SELECT id, endpoint_id, boot_id, pid, start_seconds, start_microseconds, executable_identity
           FROM process_incarnations",
    )?;
    let rows = statement
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, ProcessKey {
                endpoint_id: r.get(1)?,
                boot_id: r.get(2)?,
                pid: r.get(3)?,
                start_seconds: r.get::<_, i64>(4)?.to_string(),
                start_microseconds: r.get(5)?,
            }, r.get::<_, String>(6)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (id, key, executable) in rows {
        assign(&mut assignments, &mut seen, Entity::Process, keys::process(&key), &id);
        let images = BTreeSet::from([ProcessImage { executable: executable.clone(), point: None }]);
        state.processes.insert(
            id.clone(),
            ProcessRecord { id, key, images, current_executable: Some(executable), exited: false, created_cursor: 0, revision: 0 },
        );
    }

    // M0A's fixture linked its process only through execution_processes.
    let mut statement = conn.prepare(
        "SELECT e.id, e.session_id, e.activation, e.mode, e.presence,
                COALESCE(e.process_id, (SELECT ep.process_id FROM execution_processes ep
                                         WHERE ep.execution_id = e.id ORDER BY ep.process_id LIMIT 1)),
                e.device_number, e.started_cursor, e.ended_cursor, e.end_reason, e.surface_status
           FROM executions e",
    )?;
    type ExecRow = (String, String, i64, String, String, Option<String>, Option<i64>, Option<i64>, Option<i64>, Option<String>, Option<String>);
    let rows: Vec<ExecRow> = statement
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?))
        })?
        .collect::<Result<_, _>>()?;
    for (id, session_id, activation, mode, presence, process_id, device, started, ended, end_reason, surface_status) in rows {
        let Some(actor_id) = principal.get(&session_id).cloned() else { continue };
        let activation_ref = format!("m0:{id}");
        assign(&mut assignments, &mut seen, Entity::Execution, keys::execution(&session_id, &actor_id, &activation_ref), &id);
        let presence: ExecutionPresence = parse(&presence).unwrap_or(ExecutionPresence::Unknown);
        let attached = match presence {
            ExecutionPresence::Live => Some(AttachedPresence::Live),
            ExecutionPresence::Detached => Some(AttachedPresence::Detached),
            ExecutionPresence::Parked => Some(AttachedPresence::Parked),
            ExecutionPresence::Ended | ExecutionPresence::Unknown => None,
        };
        let end_reasons = if presence == ExecutionPresence::Ended {
            BTreeSet::from([end_reason.unwrap_or_else(|| "M0_ENDED".to_owned())])
        } else {
            BTreeSet::new()
        };
        state.executions.insert(
            id.clone(),
            ExecutionRecord {
                id,
                session_id,
                actor_id,
                activation_ref,
                process_id,
                activation: u64::try_from(activation).unwrap_or(0),
                // The M0 store's own values stand as the baseline's evidence.
                attachments: BTreeSet::new(),
                mode: parse::<ExecutionMode>(&mode),
                attached,
                native_runtime_id: None,
                controlling_device: device.and_then(|d| u32::try_from(d).ok()),
                end_reasons,
                surface_status,
                attachment_conflict: false,
                presence,
                started_cursor: started,
                ended_cursor: ended,
                created_cursor: started.unwrap_or(0),
                revision: ended.or(started).unwrap_or(0),
            },
        );
    }

    let mut statement = conn.prepare(
        "SELECT id, session_id, execution_id, surface_kind, native_locator, proof, revision, valid,
                process_id, executable_identity, device_number, terminal_generation, window_hint,
                tab_hint, evidence_observation, proof_json, invalidated_reason, invalidated_cursor
           FROM surface_bindings",
    )?;
    type BindingRow = (String, String, String, String, String, String, i64, bool, Option<String>, Option<String>, Option<i64>, Option<String>, Option<i64>, Option<i64>, Option<String>, String, Option<String>, Option<i64>);
    let rows: Vec<BindingRow> = statement
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?, r.get(15)?, r.get(16)?, r.get(17)?))
        })?
        .collect::<Result<_, _>>()?;
    for (id, session_id, execution_id, kind, locator, proof, revision, valid, process_id, executable, device, generation, window, tab, evidence, proof_json, invalidated, invalidated_cursor) in rows {
        let surface_generation = process_id
            .as_ref()
            .and_then(|p| state.processes.get(p))
            .map_or_else(|| format!("m0:{id}"), |p| format!("{}:{}.{}", p.key.pid, p.key.start_seconds, p.key.start_microseconds));
        let native = NativeSurfaceRef {
            surface_kind: kind,
            app_generation: generation.unwrap_or_else(|| "m0".to_owned()),
            locator,
            device_number: device.and_then(|d| u32::try_from(d).ok()),
            surface_generation,
        };
        let surface_key = keys::surface(&endpoint, &native);
        let surface_id = derived_id(&format!("m0-baseline-surface|{surface_key}"));
        assign(&mut assignments, &mut seen, Entity::Surface, surface_key, &surface_id);
        state.surfaces.entry(surface_id.clone()).or_insert_with(|| SourceSurfaceRecord {
            id: surface_id.clone(),
            endpoint_id: endpoint.clone(),
            native,
            created_cursor: revision,
            revision,
        });
        let binding_key = keys::binding(&execution_id, &surface_id);
        if !seen.insert(binding_key.clone()) {
            continue;
        }
        assignments.push(Assignment { native_key: binding_key, entity: Entity::Binding, id: id.clone() });
        state.bindings.insert(
            id.clone(),
            SurfaceBindingRecord {
                id,
                session_id,
                execution_id,
                surface_id,
                method: parse::<BindingMethod>(&proof),
                executable_identity: executable,
                window_hint: window,
                tab_hint: tab,
                proof: serde_json::from_str(&proof_json).unwrap_or(Value::Null),
                evidence_observation: evidence,
                proof_point: None,
                invalidations: if valid {
                    BTreeSet::new()
                } else {
                    BTreeSet::from([invalidated.unwrap_or_else(|| "M0_INVALIDATED".to_owned())])
                },
                valid,
                invalidation_reason: None,
                recorded_cursor: Some(revision),
                invalidated_cursor,
                created_cursor: revision,
                revision,
            },
        );
    }

    let mut attention_turns = BTreeSet::new();
    let mut statement = conn.prepare("SELECT turn_id FROM attention_items WHERE turn_id IS NOT NULL")?;
    for turn in statement.query_map([], |r| r.get::<_, String>(0))? {
        attention_turns.insert(turn?);
    }
    let mut statement = conn.prepare(
        "SELECT id, session_id, execution_id, native_turn_id, identity_kind, state, created_cursor FROM turns",
    )?;
    type TurnRow = (String, String, Option<String>, Option<String>, String, String, i64);
    let rows: Vec<TurnRow> = statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))?
        .collect::<Result<_, _>>()?;
    for (id, session_id, execution_id, native_turn, identity, turn_state, created) in rows {
        let Some(actor_id) = principal.get(&session_id).cloned() else { continue };
        let native_turn = native_turn.unwrap_or_else(|| format!("m0:{id}"));
        assign(&mut assignments, &mut seen, Entity::Turn, keys::turn(&session_id, &actor_id, &native_turn), &id);
        let turn_state: TurnState = parse(&turn_state).unwrap_or(TurnState::Unknown);
        let outcome = match turn_state {
            TurnState::Completed => Some(TurnOutcome::Completed),
            TurnState::Interrupted => Some(TurnOutcome::Interrupted),
            TurnState::Failed => Some(TurnOutcome::Failed),
            TurnState::Refused => Some(TurnOutcome::Refused),
            _ => None,
        };
        state.turns.insert(
            id.clone(),
            TurnRecord {
                output_ready: attention_turns.contains(&id),
                id,
                session_id,
                actor_id,
                execution_ids: execution_id.into_iter().collect(),
                native_turn_id: Some(native_turn),
                identity_kind: parse(&identity).unwrap_or(TurnIdentityKind::Native),
                owner_facing: true,
                started: matches!(turn_state, TurnState::Working | TurnState::Waiting),
                stepped: false,
                activity_seen: false,
                queued_inputs: BTreeSet::new(),
                started_by_inputs: BTreeSet::new(),
                outcomes: outcome.into_iter().collect(),
                outcome_reasons: BTreeSet::new(),
                pending_outcomes: BTreeSet::new(),
                output_points: BTreeSet::new(),
                summary: None,
                state: turn_state,
                outcome_conflict: false,
                created_cursor: created,
                revision: created,
            },
        );
    }

    let mut commands: BTreeMap<String, Vec<(String, String, Value)>> = BTreeMap::new();
    let mut statement = conn.prepare("SELECT command_id, attention_id, action, payload_json FROM attention_commands ORDER BY rowid")?;
    for row in statement.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
    })? {
        let (command_id, attention_id, action, payload) = row?;
        commands.entry(attention_id).or_default().push((command_id, action, serde_json::from_str(&payload).unwrap_or(Value::Null)));
    }
    let mut statement = conn.prepare(
        "SELECT id, session_id, turn_id, category, scope_kind, scope_key, priority, summary,
                created_by_observation, created_at_ms, acknowledged_at_ms, resolved_at_ms,
                notification_state, revision FROM attention_items",
    )?;
    type AttentionRow = (String, String, Option<String>, String, String, String, i64, Option<String>, String, i64, Option<i64>, Option<i64>, String, i64);
    let rows: Vec<AttentionRow> = statement
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?))
        })?
        .collect::<Result<_, _>>()?;
    for (id, session_id, turn_id, category, scope_kind, scope_key, priority, summary, observation, created_at, acknowledged, resolved, notification, revision) in rows {
        let scope = match scope_kind.as_str() {
            "EXACT_REQUEST" => AttentionScope::ExactRequest { native_request_id: scope_key.clone() },
            "OWNER_DECISION" => AttentionScope::OwnerDecision { decision_id: scope_key.clone() },
            _ => AttentionScope::TurnOutput { turn_id: scope_key.clone() },
        };
        let mut acknowledgements = BTreeSet::new();
        let mut resolutions = BTreeSet::new();
        let mut reason = None;
        for (command_id, action, payload) in commands.get(&id).map(Vec::as_slice).unwrap_or_default() {
            let kind = match action.as_str() {
                "AcknowledgeAttention" => OwnerActionKind::Acknowledge,
                "ResolveAttention" => OwnerActionKind::Resolve,
                _ => OwnerActionKind::Snooze,
            };
            match kind {
                OwnerActionKind::Acknowledge => {
                    acknowledgements.insert(command_id.clone());
                }
                OwnerActionKind::Resolve => {
                    let detail = payload["reason"].as_str().unwrap_or("M0 resolution").to_owned();
                    reason.get_or_insert_with(|| detail.clone());
                    resolutions.insert(ResolutionCause { kind: ResolutionKind::Owner, detail });
                }
                OwnerActionKind::Snooze => {}
            }
            state.commands.insert(
                command_id.clone(),
                CommandEffect { command_id: command_id.clone(), attention_id: id.clone(), action: kind, cursor: revision },
            );
        }
        if acknowledged.is_some() && acknowledgements.is_empty() {
            acknowledgements.insert("m0-acknowledged".to_owned());
        }
        if resolved.is_some() && resolutions.is_empty() {
            resolutions.insert(ResolutionCause { kind: ResolutionKind::Owner, detail: "M0 resolution".to_owned() });
            reason.get_or_insert_with(|| "M0 resolution".to_owned());
        }
        state.attention_by_scope.insert(keys::attention_scope(&session_id, scope.scope_kind(), scope.scope_key()), id.clone());
        let actor_id = principal.get(&session_id).cloned();
        state.attention.insert(
            id.clone(),
            AttentionRecord {
                summary_authority: if summary.is_some() { SummaryAuthority::NativeMetadata } else { SummaryAuthority::None },
                id: id.clone(),
                session_id,
                actor_id,
                turn_id,
                category: parse::<AttentionCategory>(&category).unwrap_or(AttentionCategory::TurnComplete),
                scope,
                priority: u8::try_from(priority).unwrap_or(40),
                summary,
                created_by_fact: format!("m0:{observation}"),
                created_by_observation: observation,
                created_at_ms: created_at,
                acknowledgements,
                acknowledged_at_ms: acknowledged,
                resolutions,
                resolved_at_ms: resolved,
                resolution_reason: reason,
                snoozed_until_ms: None,
                notification_state: parse::<NotificationState>(&notification).unwrap_or(NotificationState::NotRequested),
                created_cursor: revision,
                revision,
            },
        );
    }

    let mut statement = conn.prepare(
        "SELECT request_id, attention_id, state, created_at_ms, updated_at_ms, outcome_detail FROM notification_outbox",
    )?;
    let rows = statement
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?, r.get::<_, Option<String>>(5)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (request_id, attention_id, outbox_state, created, updated, detail) in rows {
        let outbox_state = match parse::<NotificationState>(&outbox_state) {
            Some(NotificationState::Pending) => OutboxState::Pending,
            Some(NotificationState::Submitted) => OutboxState::Submitted,
            Some(NotificationState::ConfirmedPresent) => OutboxState::ConfirmedPresent,
            Some(NotificationState::Uncertain) => OutboxState::Uncertain,
            Some(NotificationState::Failed) => OutboxState::Failed,
            _ => OutboxState::Suppressed,
        };
        state.outbox.insert(
            request_id.clone(),
            OutboxRecord { request_id, attention_id, state: outbox_state, detail, created_at_ms: created, updated_at_ms: updated, created_cursor: 0, revision: 0 },
        );
    }

    let mut statement = conn.prepare(
        "SELECT session_id, request_id, surface_result, session_verification, input_readiness, reason_code,
                focus_performed FROM route_results ORDER BY recorded_at_ms, rowid",
    )?;
    for row in statement.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?, r.get::<_, bool>(6)?))
    })? {
        let (session_id, request_id, surface, verification, readiness, reason_code, focus) = row?;
        if let (Some(session), Some(surface_result), Some(session_verification), Some(input_readiness)) =
            (state.sessions.get_mut(&session_id), parse(&surface), parse(&verification), parse(&readiness))
        {
            session.last_route = Some(RouteRecord { request_id, surface_result, session_verification, input_readiness, reason_code, focus_performed: focus });
        }
    }

    let mut engine = Engine::new(state);
    engine.rederive(through, &endpoint);
    Ok((engine.state, assignments))
}

/// The fingerprint the M0 journal gave an owner request: SHA-256 of its own
/// payload JSON, which a migrated command row keeps. Rendering a retried
/// request as M0 did recognises the original request; M0 had no snooze.
pub(crate) fn m0_fingerprint(command: &OwnerCommand) -> Option<String> {
    let payload = match &command.action {
        OwnerAction::Acknowledge => serde_json::json!({
            "action": "AcknowledgeAttention",
            "attentionId": command.attention_id,
            "expectedRevision": command.expected_revision,
        }),
        OwnerAction::Resolve { reason } => serde_json::json!({
            "action": "ResolveAttention",
            "attentionId": command.attention_id,
            "expectedRevision": command.expected_revision,
            "reason": reason,
        }),
        OwnerAction::Snooze { .. } => return None,
    };
    Some(sha256_hex(payload.to_string().as_bytes()))
}
