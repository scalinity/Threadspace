//! The single writer thread (SPEC §9.1, §18.4). It alone owns the journal.
//! `AttachView` captures the projection at committed cursor S, enqueues the
//! snapshot reply, then registers the view, all on this thread, so no change
//! after S can reach a connection ahead of its snapshot.

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{Receiver, Sender, SyncSender, TrySendError};
use std::thread;

use serde_json::json;
use threadspace_contracts::control::{
    ControlError, ControlErrorCode, ControlMessage, ControlOutcome, ControlResponseBody,
};
use threadspace_contracts::cursor::{format_cursor, parse_cursor};
use threadspace_contracts::diagnostics::SqliteDiagnostics;
use threadspace_contracts::projection::{
    IntentAction, IntentSource, NativeIntent, NotificationState,
};
use threadspace_journal::{Change, Journal, JournalError, NotificationIntent};
use uuid::Uuid;

use crate::bridge::{self, BridgeRequest};
use crate::log;

pub type Outbound = SyncSender<ControlMessage>;

const MAX_PENDING_INTENTS: usize = 32;

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
    #[cfg(feature = "qualification")]
    RaiseAttention {
        request_id: u64,
        label: String,
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
            Ok(patch) => patch,
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
            #[cfg(feature = "qualification")]
            WriterCommand::RaiseAttention {
                request_id,
                label,
                outbound,
            } => {
                match self
                    .journal
                    .raise_qualification_attention(&label, log::now_ms())
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
        }
    }

    /// Re-reads the item on every click (SPEC §7.5), queues a validated intent
    /// until a view has applied its snapshot, and brings the office forward.
    fn notification_response(&mut self, notification_request_id: &str, attention_id: &str) {
        if !valid_uuid(attention_id) {
            log::warn(
                "NOTIFICATION_RESPONSE_REJECTED",
                json!({ "reason": "attention ID is not a UUID" }),
            );
            return;
        }
        match self.journal.attention_target(attention_id) {
            Ok(target) => {
                let intent = NativeIntent {
                    intent_id: Uuid::new_v4().to_string(),
                    action: IntentAction::OpenAttention {
                        attention_id: target.attention_id,
                        session_id: target.session_id,
                        outstanding: target.outstanding,
                        source: IntentSource::NotificationResponse,
                    },
                };
                log::info(
                    "NOTIFICATION_RESPONSE",
                    json!({
                        "notificationRequestId": notification_request_id,
                        "attentionId": attention_id,
                        "outstanding": target.outstanding,
                        "intentId": intent.intent_id,
                        "hydratedViews": self.views.values().filter(|view| view.hydrated).count(),
                    }),
                );
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
            Err(error) => log::warn(
                "NOTIFICATION_RESPONSE_TARGET_MISSING",
                json!({ "attentionId": attention_id, "error": error.to_string() }),
            ),
        }
        bridge::notify(&BridgeRequest::OpenContainingApp);
        log::info(
            "CONTAINING_APP_OPEN_REQUESTED",
            json!({ "attentionId": attention_id }),
        );
    }
}

pub fn spawn(
    journal: Journal,
    commands: Receiver<WriterCommand>,
    notifier: SyncSender<NotificationIntent>,
) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("journal-writer".into())
        .spawn(move || {
            let last_cursor = journal.cursor().unwrap_or(0);
            let mut writer = Writer {
                journal,
                views: HashMap::new(),
                intents: VecDeque::new(),
                last_cursor,
                notifier,
            };
            for command in commands {
                writer.handle(command);
            }
            log::warn("WRITER_STOPPED", json!({}));
        })
}
