//! The single writer thread (SPEC §9.1, §18.4). It alone owns the journal.
//! `AttachView` captures the projection at committed cursor S, enqueues the
//! snapshot reply, then registers the view, all on this thread, so no change
//! after S can reach a connection ahead of its snapshot.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, SyncSender, TrySendError};
use std::thread;

use serde_json::json;
use threadspace_contracts::control::{
    ControlError, ControlErrorCode, ControlMessage, ControlOutcome, ControlResponseBody,
    MaintenancePhase, MaintenancePurpose, MaintenanceReport,
};
use threadspace_contracts::cursor::{format_cursor, parse_cursor};
use threadspace_contracts::diagnostics::{ProcessIdentity, SqliteDiagnostics};
use threadspace_contracts::limits::FRAME_MAX_BYTES;
use threadspace_contracts::projection::{
    EntityKind, IntentAction, IntentSource, NativeIntent, NotificationState, ProjectionPatch,
};
use threadspace_contracts::route::RouteResult;
use threadspace_contracts::ui::{AttentionPage, FleetPage};
use threadspace_journal::{
    ApplyOutcome, Change, DiscoveryApplication, Journal, JournalError, LiveExecutionRow,
    NotificationIntent, RouteTargetRow,
};
use uuid::Uuid;

use crate::bridge::{self, BridgeRequest};
use crate::log;
use crate::respond::ResponseJob;
use crate::state::RUNTIME;

pub type Outbound = SyncSender<ControlMessage>;

const MAX_PENDING_INTENTS: usize = 32;
/// A patch larger than this is sent as page invalidations plus counts, so
/// one change can never exceed the 64 KiB frame bound (SPEC §18.4).
const PATCH_UPSERT_BUDGET: usize = FRAME_MAX_BYTES - 4096;

pub enum WriterCommand {
    AttachView {
        connection_id: u64,
        request_id: u64,
        subscription_id: String,
        outbound: Outbound,
    },
    DetachView {
        request_id: u64,
        subscription_id: String,
        outbound: Outbound,
    },
    ViewHydrated {
        request_id: u64,
        subscription_id: String,
        outbound: Outbound,
    },
    IntentConsumed {
        request_id: u64,
        intent_id: String,
        outbound: Outbound,
    },
    ConnectionClosed {
        connection_id: u64,
    },
    Acknowledge {
        request_id: u64,
        command_id: String,
        attention_id: String,
        expected_revision: Option<String>,
        outbound: Outbound,
    },
    Resolve {
        request_id: u64,
        command_id: String,
        attention_id: String,
        expected_revision: Option<String>,
        reason: String,
        outbound: Outbound,
    },
    Page {
        request_id: u64,
        attention: bool,
        after: Option<String>,
        limit: u32,
        outbound: Outbound,
    },
    SetObservationEnabled {
        request_id: u64,
        enabled: bool,
        outbound: Outbound,
    },
    PrepareMaintenance {
        request_id: u64,
        purpose: MaintenancePurpose,
        outbound: Outbound,
    },
    CancelMaintenance {
        request_id: u64,
        outbound: Outbound,
    },
    /// Queue a validated intent for hydrated views, optionally bringing the
    /// containing application forward.
    PushIntent {
        intent: NativeIntent,
        open_app: bool,
    },
    /// A sleep/wake transition (SPEC §19.5), journaled as a lifecycle fact.
    RecordLifecycle {
        native_event: &'static str,
        payload: serde_json::Value,
    },
    SqliteDiagnostics {
        reply: Sender<Result<SqliteDiagnostics, String>>,
    },
    NotificationOutcome {
        request_id: String,
        state: NotificationState,
        detail: String,
    },
    NotificationResponse {
        notification_request_id: String,
        attention_id: String,
    },
    LiveExecutions {
        provider: &'static str,
        reply: Sender<Result<Vec<LiveExecutionRow>, String>>,
    },
    ApplyDiscovery {
        application: Box<DiscoveryApplication>,
        reply: Sender<Result<ApplyOutcome, String>>,
    },
    RouteTarget {
        session_id: String,
        reply: Sender<Result<RouteTargetRow, String>>,
    },
    BindingRevision {
        binding_id: String,
        reply: Sender<Option<i64>>,
    },
    RecordRoute {
        result: Box<RouteResult>,
        reply: Sender<Result<i64, String>>,
    },
    #[cfg(feature = "qualification")]
    ExportObservations {
        after_cursor: i64,
        limit: u32,
        reply: Sender<Result<Vec<threadspace_journal::ObservationExport>, String>>,
    },
    #[cfg(feature = "qualification")]
    RaiseAttention {
        request_id: u64,
        label: String,
        session_id: Option<String>,
        outbound: Outbound,
    },
    #[cfg(feature = "qualification")]
    SyntheticChange {
        run_id: String,
        slot: u32,
        sequence: u64,
        reply: Option<Sender<Result<i64, String>>>,
    },
    #[cfg(feature = "qualification")]
    Populate {
        request_id: u64,
        sessions: u32,
        name_bytes: u32,
        outbound: Outbound,
    },
    #[cfg(feature = "qualification")]
    ViewCommand {
        request_id: u64,
        command: String,
        args: serde_json::Value,
        outbound: Outbound,
    },
    #[cfg(feature = "qualification")]
    Admit {
        request_id: u64,
        observation_id: String,
        outbound: Outbound,
    },
}

struct View {
    connection_id: u64,
    outbound: Outbound,
    hydrated: bool,
}

struct Writer {
    journal: Journal,
    views: HashMap<String, View>,
    intents: VecDeque<NativeIntent>,
    last_cursor: i64,
    responder: SyncSender<ResponseJob>,
    identity: ProcessIdentity,
    store_dir: PathBuf,
    // In M0A only qualification attention reaches the outbox; provider-derived
    // attention feeds it once the canonical reducer exists (M1/M5).
    #[cfg_attr(not(feature = "qualification"), allow(dead_code))]
    notifier: SyncSender<NotificationIntent>,
}

fn journal_error(error: &JournalError) -> ControlError {
    let code = match error {
        JournalError::NotFound { .. } => ControlErrorCode::NotFound,
        JournalError::Conflict { .. } => ControlErrorCode::Conflict,
        JournalError::Invalid { .. } => ControlErrorCode::BadRequest,
        _ => ControlErrorCode::Internal,
    };
    ControlError::new(code, error.to_string())
}

fn bad_request(detail: &str) -> ControlError {
    ControlError::new(ControlErrorCode::BadRequest, detail)
}

fn gated() -> ControlError {
    ControlError::new(
        ControlErrorCode::MaintenanceGated,
        "maintenance holds the store; admission is closed",
    )
}

/// Replaces entity upserts with page invalidations when the patch would not
/// fit one frame; counts stay exact.
fn fit_patch(mut patch: ProjectionPatch) -> ProjectionPatch {
    let size = serde_json::to_vec(&patch)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX);
    if size <= PATCH_UPSERT_BUDGET {
        return patch;
    }
    if !patch.session_upserts.is_empty() {
        patch.page_invalidations.push(EntityKind::Session);
    }
    if !patch.attention_upserts.is_empty() {
        patch.page_invalidations.push(EntityKind::Attention);
    }
    patch.session_upserts.clear();
    patch.attention_upserts.clear();
    patch
}

fn valid_uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|parsed| parsed.hyphenated().to_string() == value)
}

pub fn respond(
    outbound: &Outbound,
    request_id: u64,
    outcome: Result<ControlResponseBody, ControlError>,
) {
    let outcome = match outcome {
        Ok(body) => ControlOutcome::Ok(Box::new(body)),
        Err(error) => ControlOutcome::Err(error),
    };
    if let Err(error) = outbound.try_send(ControlMessage::Response {
        request_id,
        outcome,
    }) {
        log::warn(
            "RESPONSE_DROPPED",
            json!({ "requestId": request_id, "full": matches!(error, TrySendError::Full(_)) }),
        );
    }
}

impl Writer {
    fn broadcast(&mut self, change: &Change) {
        let patch = match self.journal.patch_for(self.last_cursor, change) {
            Ok(patch) => fit_patch(patch),
            Err(error) => {
                log::error("PATCH_BUILD_FAILED", json!({ "error": error.to_string() }));
                return;
            }
        };
        self.last_cursor = change.cursor;
        let cursor = format_cursor(change.cursor);
        let mut retired = Vec::new();
        for (subscription_id, view) in &self.views {
            let message = ControlMessage::ViewPatch {
                subscription_id: subscription_id.clone(),
                cursor: cursor.clone(),
                patch: Box::new(patch.clone()),
            };
            if view.outbound.try_send(message).is_err() {
                retired.push(subscription_id.clone());
            }
        }
        for subscription_id in retired {
            log::warn(
                "VIEW_RETIRED_BACKPRESSURE",
                json!({ "subscriptionId": subscription_id }),
            );
            self.views.remove(&subscription_id);
        }
        log::info(
            "PATCH_BROADCAST",
            json!({ "cursor": cursor, "views": self.views.len() }),
        );
    }

    fn push_intents(&mut self, subscription_id: &str) {
        let Some(view) = self.views.get(subscription_id) else {
            return;
        };
        if !view.hydrated {
            return;
        }
        for intent in &self.intents {
            let message = ControlMessage::Intent {
                subscription_id: subscription_id.to_owned(),
                cursor: format_cursor(self.last_cursor),
                intent: intent.clone(),
            };
            if view.outbound.try_send(message).is_ok() {
                log::info(
                    "INTENT_DELIVERED",
                    json!({ "intentId": intent.intent_id, "subscriptionId": subscription_id }),
                );
            }
        }
    }

    fn handle(&mut self, command: WriterCommand) {
        match command {
            WriterCommand::AttachView {
                connection_id,
                request_id,
                subscription_id,
                outbound,
            } => {
                if !valid_uuid(&subscription_id) || self.views.contains_key(&subscription_id) {
                    respond(
                        &outbound,
                        request_id,
                        Err(bad_request("subscription ID must be a new UUID")),
                    );
                    return;
                }
                match self.journal.snapshot() {
                    Ok((cursor, snapshot)) => {
                        self.last_cursor = cursor;
                        respond(
                            &outbound,
                            request_id,
                            Ok(ControlResponseBody::ViewAttached {
                                subscription_id: subscription_id.clone(),
                                cursor: format_cursor(cursor),
                                snapshot,
                            }),
                        );
                        log::info(
                            "VIEW_ATTACHED",
                            json!({ "subscriptionId": subscription_id, "cursor": cursor }),
                        );
                        self.views.insert(
                            subscription_id,
                            View {
                                connection_id,
                                outbound,
                                hydrated: false,
                            },
                        );
                    }
                    Err(error) => respond(&outbound, request_id, Err(journal_error(&error))),
                }
            }
            WriterCommand::DetachView {
                request_id,
                subscription_id,
                outbound,
            } => {
                self.views.remove(&subscription_id);
                respond(&outbound, request_id, Ok(ControlResponseBody::Done));
            }
            WriterCommand::ViewHydrated {
                request_id,
                subscription_id,
                outbound,
            } => {
                let Some(view) = self.views.get_mut(&subscription_id) else {
                    respond(
                        &outbound,
                        request_id,
                        Err(ControlError::new(
                            ControlErrorCode::UnknownSubscription,
                            "unknown subscription",
                        )),
                    );
                    return;
                };
                view.hydrated = true;
                respond(&outbound, request_id, Ok(ControlResponseBody::Done));
                log::info(
                    "VIEW_HYDRATED",
                    json!({ "subscriptionId": subscription_id, "pendingIntents": self.intents.len() }),
                );
                self.push_intents(&subscription_id);
            }
            WriterCommand::IntentConsumed {
                request_id,
                intent_id,
                outbound,
            } => {
                let before = self.intents.len();
                self.intents.retain(|intent| intent.intent_id != intent_id);
                if self.intents.len() < before {
                    log::info("INTENT_CONSUMED", json!({ "intentId": intent_id }));
                }
                respond(&outbound, request_id, Ok(ControlResponseBody::Done));
            }
            WriterCommand::ConnectionClosed { connection_id } => {
                self.views
                    .retain(|_, view| view.connection_id != connection_id);
            }
            WriterCommand::Acknowledge {
                request_id,
                command_id,
                attention_id,
                expected_revision,
                outbound,
            } => {
                if !valid_uuid(&command_id) || !valid_uuid(&attention_id) {
                    respond(
                        &outbound,
                        request_id,
                        Err(bad_request("command and attention IDs must be UUIDs")),
                    );
                    return;
                }
                if !RUNTIME.writes_open() {
                    respond(&outbound, request_id, Err(gated()));
                    return;
                }
                let expected = match expected_revision.as_deref().map(parse_cursor) {
                    Some(None) => {
                        respond(
                            &outbound,
                            request_id,
                            Err(bad_request("expected revision must be a cursor")),
                        );
                        return;
                    }
                    Some(Some(value)) => Some(value),
                    None => None,
                };
                match self.journal.acknowledge_attention(
                    &command_id,
                    &attention_id,
                    expected,
                    log::now_ms(),
                ) {
                    Ok(outcome) => {
                        log::info(
                            "OWNER_COMMAND_RECEIPT",
                            json!({ "commandId": command_id, "status": outcome.receipt.status, "cursor": outcome.receipt.cursor }),
                        );
                        respond(
                            &outbound,
                            request_id,
                            Ok(ControlResponseBody::CommandReceipt {
                                receipt: outcome.receipt,
                            }),
                        );
                        if let Some(change) = outcome.change {
                            self.broadcast(&change);
                        }
                    }
                    Err(error) => respond(&outbound, request_id, Err(journal_error(&error))),
                }
            }
            WriterCommand::Resolve {
                request_id,
                command_id,
                attention_id,
                expected_revision,
                reason,
                outbound,
            } => {
                let reason = reason.trim();
                if !valid_uuid(&command_id) || !valid_uuid(&attention_id) || reason.is_empty() {
                    respond(
                        &outbound,
                        request_id,
                        Err(bad_request(
                            "UUID command/attention IDs and a reason are required",
                        )),
                    );
                    return;
                }
                if !RUNTIME.writes_open() {
                    respond(&outbound, request_id, Err(gated()));
                    return;
                }
                let expected = match expected_revision.as_deref().map(parse_cursor) {
                    Some(None) => {
                        respond(
                            &outbound,
                            request_id,
                            Err(bad_request("expected revision must be a cursor")),
                        );
                        return;
                    }
                    Some(Some(value)) => Some(value),
                    None => None,
                };
                let reason: String = reason
                    .chars()
                    .take(threadspace_contracts::limits::LABEL_MAX_CHARS)
                    .collect();
                match self.journal.resolve_attention(
                    &command_id,
                    &attention_id,
                    expected,
                    &reason,
                    log::now_ms(),
                ) {
                    Ok(outcome) => {
                        log::info(
                            "OWNER_COMMAND_RECEIPT",
                            json!({ "commandId": command_id, "action": "ResolveAttention", "status": outcome.receipt.status, "cursor": outcome.receipt.cursor }),
                        );
                        respond(
                            &outbound,
                            request_id,
                            Ok(ControlResponseBody::CommandReceipt {
                                receipt: outcome.receipt,
                            }),
                        );
                        if let Some(change) = outcome.change {
                            self.broadcast(&change);
                        }
                    }
                    Err(error) => respond(&outbound, request_id, Err(journal_error(&error))),
                }
            }
            WriterCommand::Page {
                request_id,
                attention,
                after,
                limit,
                outbound,
            } => {
                if after.as_deref().is_some_and(|value| !valid_uuid(value)) || limit == 0 {
                    respond(
                        &outbound,
                        request_id,
                        Err(bad_request("after must be a UUID and limit positive")),
                    );
                    return;
                }
                let limit = limit.min(500);
                let outcome = if attention {
                    self.journal
                        .attention_page(after.as_deref(), limit)
                        .map(|(cursor, page)| ControlResponseBody::AttentionPage {
                            page: AttentionPage {
                                view_revision: format_cursor(cursor),
                                rows: page.rows,
                                next_after: page.next_after,
                                total: page.total,
                            },
                        })
                } else {
                    self.journal
                        .session_page(after.as_deref(), limit)
                        .map(|(cursor, page)| ControlResponseBody::FleetPage {
                            page: FleetPage {
                                view_revision: format_cursor(cursor),
                                rows: page.rows,
                                next_after: page.next_after,
                                total: page.total,
                            },
                        })
                };
                respond(
                    &outbound,
                    request_id,
                    outcome.map_err(|error| journal_error(&error)),
                );
            }
            WriterCommand::SetObservationEnabled {
                request_id,
                enabled,
                outbound,
            } => match self.journal.set_observation_enabled(enabled, log::now_ms()) {
                Ok(cursor) => {
                    RUNTIME.set_observation_enabled(enabled);
                    log::info(
                        "OBSERVATION_PREFERENCE",
                        json!({ "enabled": enabled, "cursor": cursor }),
                    );
                    respond(&outbound, request_id, Ok(ControlResponseBody::Done));
                }
                Err(error) => respond(&outbound, request_id, Err(journal_error(&error))),
            },
            WriterCommand::PrepareMaintenance {
                request_id,
                purpose,
                outbound,
            } => {
                let outcome = self.prepare_maintenance(&purpose);
                respond(&outbound, request_id, outcome);
            }
            WriterCommand::CancelMaintenance {
                request_id,
                outbound,
            } => {
                match self.journal.record_maintenance_phase(
                    "NONE",
                    None,
                    json!({ "reason": "cancelled" }),
                    log::now_ms(),
                ) {
                    Ok(cursor) => {
                        RUNTIME.set_maintenance(MaintenancePhase::None);
                        log::info("MAINTENANCE_CANCELLED", json!({ "cursor": cursor }));
                        respond(&outbound, request_id, Ok(ControlResponseBody::Done));
                    }
                    Err(error) => respond(&outbound, request_id, Err(journal_error(&error))),
                }
            }
            WriterCommand::PushIntent { intent, open_app } => {
                self.queue_intent(intent);
                if open_app {
                    bridge::notify(&BridgeRequest::OpenContainingApp);
                }
            }
            WriterCommand::RecordLifecycle {
                native_event,
                payload,
            } => {
                if !RUNTIME.writes_open() {
                    log::info(
                        "LIFECYCLE_NOT_JOURNALED",
                        json!({ "event": native_event, "reason": "maintenance" }),
                    );
                    return;
                }
                match self
                    .journal
                    .record_lifecycle(native_event, payload, log::now_ms())
                {
                    Ok(cursor) => log::info(
                        "LIFECYCLE_RECORDED",
                        json!({ "event": native_event, "cursor": cursor }),
                    ),
                    Err(error) => log::warn(
                        "LIFECYCLE_UNRECORDED",
                        json!({ "event": native_event, "error": error.to_string() }),
                    ),
                }
            }
            WriterCommand::SqliteDiagnostics { reply } => {
                let _ = reply.send(
                    self.journal
                        .sqlite_diagnostics()
                        .map_err(|error| error.to_string()),
                );
            }
            WriterCommand::NotificationOutcome {
                request_id,
                state,
                detail,
            } => {
                match self.journal.record_notification_state(
                    &request_id,
                    &state,
                    &detail,
                    log::now_ms(),
                ) {
                    Ok(change) => self.broadcast(&change),
                    Err(error) => log::error(
                        "NOTIFICATION_OUTCOME_UNRECORDED",
                        json!({ "requestId": request_id, "error": error.to_string() }),
                    ),
                }
            }
            WriterCommand::NotificationResponse {
                notification_request_id,
                attention_id,
            } => {
                self.notification_response(&notification_request_id, &attention_id);
            }
            WriterCommand::LiveExecutions { provider, reply } => {
                let _ = reply.send(
                    self.journal
                        .live_executions(provider)
                        .map_err(|error| error.to_string()),
                );
            }
            WriterCommand::ApplyDiscovery { application, reply } => {
                if !RUNTIME.admission_open() {
                    let _ = reply.send(Err("capture admission is closed".into()));
                    return;
                }
                match self.journal.apply_discovery(&application, log::now_ms()) {
                    Ok(outcome) => {
                        if let Some(change) = &outcome.change {
                            self.broadcast(change);
                        }
                        let _ = reply.send(Ok(outcome));
                    }
                    Err(error) => {
                        log::error(
                            "DISCOVERY_APPLY_FAILED",
                            json!({ "error": error.to_string() }),
                        );
                        let _ = reply.send(Err(error.to_string()));
                    }
                }
            }
            WriterCommand::RouteTarget { session_id, reply } => {
                let _ = reply.send(
                    self.journal
                        .route_target(&session_id)
                        .map_err(|error| error.to_string()),
                );
            }
            WriterCommand::BindingRevision { binding_id, reply } => {
                let _ = reply.send(self.journal.binding_revision(&binding_id).ok().flatten());
            }
            WriterCommand::RecordRoute { result, reply } => {
                if !RUNTIME.writes_open() {
                    let _ = reply.send(Err("maintenance holds the store".into()));
                    return;
                }
                match self.journal.record_route(&result, log::now_ms()) {
                    Ok(change) => {
                        let cursor = change.cursor;
                        self.broadcast(&change);
                        let _ = reply.send(Ok(cursor));
                    }
                    Err(error) => {
                        log::error(
                            "ROUTE_RECORD_FAILED",
                            json!({ "requestId": result.request_id, "error": error.to_string() }),
                        );
                        let _ = reply.send(Err(error.to_string()));
                    }
                }
            }
            #[cfg(feature = "qualification")]
            WriterCommand::ExportObservations {
                after_cursor,
                limit,
                reply,
            } => {
                let _ = reply.send(
                    self.journal
                        .export_observations(after_cursor, limit)
                        .map_err(|error| error.to_string()),
                );
            }
            #[cfg(feature = "qualification")]
            WriterCommand::RaiseAttention {
                request_id,
                label,
                session_id,
                outbound,
            } => {
                if !RUNTIME.writes_open() {
                    respond(&outbound, request_id, Err(gated()));
                    return;
                }
                if session_id.as_deref().is_some_and(|id| !valid_uuid(id)) {
                    respond(
                        &outbound,
                        request_id,
                        Err(bad_request("sessionId must be a UUID")),
                    );
                    return;
                }
                match self
                    .journal
                    .raise_attention_on(&label, session_id.as_deref(), log::now_ms())
                {
                    Ok(raised) => {
                        log::info(
                            "QUALIFICATION_ATTENTION_RAISED",
                            json!({ "attentionId": raised.intent.attention_id, "requestId": raised.intent.request_id, "cursor": raised.change.cursor }),
                        );
                        respond(
                            &outbound,
                            request_id,
                            Ok(ControlResponseBody::AttentionRaised {
                                attention_id: raised.intent.attention_id.clone(),
                                session_id: raised.intent.session_id.clone(),
                                notification_request_id: raised.intent.request_id.clone(),
                                cursor: format_cursor(raised.change.cursor),
                            }),
                        );
                        self.broadcast(&raised.change);
                        // OS side effects run only after commit (SPEC §5.5).
                        if self.notifier.try_send(raised.intent).is_err() {
                            log::warn("NOTIFIER_BACKLOGGED", json!({}));
                        }
                    }
                    Err(error) => respond(&outbound, request_id, Err(journal_error(&error))),
                }
            }
            #[cfg(feature = "qualification")]
            WriterCommand::SyntheticChange {
                run_id,
                slot,
                sequence,
                reply,
            } => {
                let outcome = if RUNTIME.writes_open() {
                    self.journal
                        .synthetic_change(&run_id, slot, sequence, log::now_ms())
                        .map_err(|error| error.to_string())
                } else {
                    Err("maintenance holds the store".to_owned())
                };
                match outcome {
                    Ok(change) => {
                        let cursor = change.cursor;
                        self.broadcast(&change);
                        if let Some(reply) = reply {
                            let _ = reply.send(Ok(cursor));
                        }
                    }
                    Err(error) => {
                        log::warn(
                            "SYNTHETIC_CHANGE_FAILED",
                            json!({ "runId": run_id, "sequence": sequence, "error": error }),
                        );
                        if let Some(reply) = reply {
                            let _ = reply.send(Err(error));
                        }
                    }
                }
            }
            #[cfg(feature = "qualification")]
            WriterCommand::Populate {
                request_id,
                sessions,
                name_bytes,
                outbound,
            } => {
                if !RUNTIME.writes_open() {
                    respond(&outbound, request_id, Err(gated()));
                    return;
                }
                match self.journal.populate_synthetic(
                    sessions.min(20_000),
                    name_bytes.min(4096),
                    log::now_ms(),
                ) {
                    Ok(change) => {
                        respond(
                            &outbound,
                            request_id,
                            Ok(ControlResponseBody::Populated {
                                sessions: change.session_ids.len() as u32,
                                cursor: format_cursor(change.cursor),
                            }),
                        );
                        self.broadcast(&change);
                    }
                    Err(error) => respond(&outbound, request_id, Err(journal_error(&error))),
                }
            }
            #[cfg(feature = "qualification")]
            WriterCommand::Admit {
                request_id,
                observation_id,
                outbound,
            } => {
                if !RUNTIME.writes_open() {
                    respond(&outbound, request_id, Err(gated()));
                    return;
                }
                let payload = json!({ "fixture": "durability" });
                let epoch = self.journal.store_generation().to_owned();
                let admitted = self.journal.admit_observation(
                    &threadspace_journal::ObservationAdmission {
                        observation_id: &observation_id,
                        source_id: "qualification.durability",
                        source_epoch: &epoch,
                        source_sequence: None,
                        native_event: "QUALIFY_DURABILITY_RECORD",
                        captured_wall_ms: log::now_ms(),
                        payload: &payload,
                    },
                    log::now_ms(),
                );
                respond(
                    &outbound,
                    request_id,
                    admitted
                        .map(|receipt| ControlResponseBody::Admitted {
                            observation_id: receipt.observation_id,
                            status: receipt.status,
                            cursor: format_cursor(receipt.cursor),
                        })
                        .map_err(|error| journal_error(&error)),
                );
            }
            #[cfg(feature = "qualification")]
            WriterCommand::ViewCommand {
                request_id,
                command,
                args,
                outbound,
            } => {
                let intent = NativeIntent {
                    intent_id: Uuid::new_v4().to_string(),
                    action: IntentAction::QualificationCommand {
                        command: command.clone(),
                        args,
                    },
                };
                let intent_id = intent.intent_id.clone();
                let hydrated = self.views.values().filter(|view| view.hydrated).count() as u32;
                self.queue_intent(intent);
                log::info(
                    "QUALIFICATION_VIEW_COMMAND",
                    json!({ "command": command, "intentId": intent_id, "hydratedViews": hydrated }),
                );
                respond(
                    &outbound,
                    request_id,
                    Ok(ControlResponseBody::ViewCommandQueued {
                        intent_id,
                        hydrated_views: hydrated,
                    }),
                );
            }
        }
    }

    /// Queues an intent until a view has applied its snapshot and delivers it
    /// to every hydrated view.
    fn queue_intent(&mut self, intent: NativeIntent) {
        if self.intents.len() >= MAX_PENDING_INTENTS {
            self.intents.pop_front();
        }
        self.intents.push_back(intent);
        let hydrated: Vec<String> = self
            .views
            .iter()
            .filter(|(_, view)| view.hydrated)
            .map(|(id, _)| id.clone())
            .collect();
        for subscription_id in hydrated {
            self.push_intents(&subscription_id);
        }
    }

    /// Re-reads the item on every click (SPEC §7.5). An outstanding item with
    /// observation enabled gets the verified current-session Return (planned
    /// off this thread); anything else opens the inspector. Nothing here
    /// acknowledges, resolves or starts capture.
    fn notification_response(&mut self, notification_request_id: &str, attention_id: &str) {
        if !valid_uuid(attention_id) {
            log::warn(
                "NOTIFICATION_RESPONSE_REJECTED",
                json!({ "reason": "attention ID is not a UUID" }),
            );
            return;
        }
        let target = match self.journal.attention_target(attention_id) {
            Ok(target) => target,
            Err(error) => {
                log::warn(
                    "NOTIFICATION_RESPONSE_TARGET_MISSING",
                    json!({ "attentionId": attention_id, "error": error.to_string() }),
                );
                bridge::notify(&BridgeRequest::OpenContainingApp);
                return;
            }
        };
        let observing = RUNTIME.admission_open();
        let plan = if target.outstanding && observing {
            "RETURN"
        } else {
            "INSPECTOR"
        };
        log::info(
            "NOTIFICATION_RESPONSE",
            json!({
                "notificationRequestId": notification_request_id,
                "attentionId": attention_id,
                "sessionId": target.session_id,
                "outstanding": target.outstanding,
                "observationEnabled": RUNTIME.observation_enabled(),
                "maintenance": RUNTIME.maintenance(),
                "plan": plan,
                "hydratedViews": self.views.values().filter(|view| view.hydrated).count(),
            }),
        );
        if plan == "RETURN" {
            let job = ResponseJob::Return {
                notification_request_id: notification_request_id.to_owned(),
                attention_id: target.attention_id.clone(),
                session_id: target.session_id.clone(),
            };
            if self.responder.try_send(job).is_ok() {
                return;
            }
            log::warn(
                "NOTIFICATION_RETURN_NOT_QUEUED",
                json!({ "attentionId": attention_id }),
            );
        }
        let intent = NativeIntent {
            intent_id: Uuid::new_v4().to_string(),
            action: IntentAction::OpenAttention {
                attention_id: target.attention_id,
                session_id: target.session_id,
                outstanding: target.outstanding,
                source: IntentSource::NotificationResponse,
                route: None,
                observation_enabled: observing,
            },
        };
        log::info(
            "NOTIFICATION_INSPECTOR",
            json!({ "attentionId": attention_id, "intentId": intent.intent_id }),
        );
        self.queue_intent(intent);
        bridge::notify(&BridgeRequest::OpenContainingApp);
        log::info(
            "CONTAINING_APP_OPEN_REQUESTED",
            json!({ "attentionId": attention_id }),
        );
    }

    /// SPEC §19.5 preparation: record PREPARING (closing admission), finish
    /// already accepted work (everything queued before this command on the
    /// single writer has run), create a verified consistent backup, then
    /// record PREPARED and stay alive holding the writer lock. A failure
    /// reopens admission and reports the error, so the caller never
    /// unregisters a helper that did not prepare.
    fn prepare_maintenance(
        &mut self,
        purpose: &MaintenancePurpose,
    ) -> Result<ControlResponseBody, ControlError> {
        if RUNTIME.maintenance() != MaintenancePhase::None {
            return Err(ControlError::new(
                ControlErrorCode::Conflict,
                "a maintenance phase is already active",
            ));
        }
        let purpose_json = serde_json::to_string(purpose)
            .map_err(|error| ControlError::new(ControlErrorCode::Internal, error.to_string()))?;
        self.journal
            .record_maintenance_phase("PREPARING", Some(&purpose_json), json!({}), log::now_ms())
            .map_err(|error| journal_error(&error))?;
        RUNTIME.set_maintenance(MaintenancePhase::Preparing);
        log::info("MAINTENANCE_PREPARING", json!({ "purpose": purpose }));
        let backup = if RUNTIME.take_backup_failure() {
            Err("qualification fault: backup failed".to_owned())
        } else {
            crate::maintenance::consistent_backup(&mut self.journal, &self.store_dir)
        };
        let backup = match backup {
            Ok(backup) => backup,
            Err(error) => {
                log::error("MAINTENANCE_PREPARE_FAILED", json!({ "error": error }));
                let reopened = self.journal.record_maintenance_phase(
                    "NONE",
                    None,
                    json!({ "reason": "preparation failed", "error": error }),
                    log::now_ms(),
                );
                if reopened.is_ok() {
                    RUNTIME.set_maintenance(MaintenancePhase::None);
                }
                return Err(ControlError::new(
                    ControlErrorCode::Internal,
                    format!("maintenance preparation failed: {error}"),
                ));
            }
        };
        let prepared_at_ms = log::now_ms();
        self.journal
            .record_maintenance_phase(
                "PREPARED",
                Some(&purpose_json),
                json!({ "backupFile": backup.file_name, "backupCursor": backup.cursor, "backupSha256": backup.sha256 }),
                prepared_at_ms,
            )
            .map_err(|error| journal_error(&error))?;
        RUNTIME.set_maintenance(MaintenancePhase::Prepared);
        log::info(
            "MAINTENANCE_PREPARED",
            json!({ "backupFile": backup.file_name, "backupCursor": backup.cursor, "backupBytes": backup.bytes }),
        );
        let mut companion = self.identity.clone();
        companion.executable_path =
            threadspace_relay::paths::redact_home(&companion.executable_path);
        Ok(ControlResponseBody::MaintenancePrepared {
            report: Box::new(MaintenanceReport {
                phase: MaintenancePhase::Prepared,
                purpose: purpose.clone(),
                backup_file: backup.file_name,
                backup_cursor: format_cursor(backup.cursor),
                backup_sha256: backup.sha256,
                backup_bytes: backup.bytes,
                prepared_at_ms,
                companion,
            }),
        })
    }
}

pub struct WriterSetup {
    pub journal: Journal,
    pub commands: Receiver<WriterCommand>,
    pub notifier: SyncSender<NotificationIntent>,
    pub responder: SyncSender<ResponseJob>,
    pub identity: ProcessIdentity,
    pub store_dir: PathBuf,
}

pub fn spawn(setup: WriterSetup) -> std::io::Result<thread::JoinHandle<()>> {
    let WriterSetup {
        journal,
        commands,
        notifier,
        responder,
        identity,
        store_dir,
    } = setup;
    thread::Builder::new()
        .name("journal-writer".into())
        .spawn(move || {
            let last_cursor = journal.cursor().unwrap_or(0);
            let mut writer = Writer {
                journal,
                views: HashMap::new(),
                intents: VecDeque::new(),
                last_cursor,
                responder,
                identity,
                store_dir,
                notifier,
            };
            for command in commands {
                writer.handle(command);
            }
            log::warn("WRITER_STOPPED", json!({}));
        })
}
