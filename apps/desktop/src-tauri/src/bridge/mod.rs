//! The renderer bridge: managed Tauri state holding the companion link, the
//! subscription registry and the native view registry. It is a transport and
//! validation layer, not a second state engine (SPEC §18.2).

pub mod link;
pub mod stream;

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use threadspace_contracts::control::{
    ControlErrorCode, ControlMessage, ControlRequestBody, ControlResponseBody,
};
use threadspace_contracts::cursor::parse_cursor;
use threadspace_contracts::limits::QUERY_MAX_IN_FLIGHT;
use threadspace_contracts::ui::{
    StreamLimits, UI_PROTOCOL_VERSION, UiAckReply, UiAckRequest, UiCallContext, UiConnectReply,
    UiConnectRequest, UiDisconnectRequest, UiError, UiErrorCode, UiFrame, parse_uuid,
};
use threadspace_relay::paths::AgentPaths;
use uuid::Uuid;

use crate::incarnation::ViewRegistry;
use link::{CompanionLink, LinkError};
use stream::{AckError, FrameIdentity, StreamError, StreamSender};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// A query/command reply this large went through the framework's per-view
/// fetch cache; one issued this recently may still be unconsumed there.
const LARGE_REPLY_BYTES: usize = stream::CHANNEL_CACHE_THRESHOLD;
const LARGE_REPLY_WINDOW: Duration = Duration::from_secs(30);

/// Asks the shell to retire and recreate the office view (SPEC §18.5).
pub type RecoveryHook = Box<dyn Fn(Uuid, &'static str) + Send + Sync>;
/// Writes one native event to the shell log.
pub type EventLog = Box<dyn Fn(&str, serde_json::Value) + Send + Sync>;
/// Bounded memory of epochs already used; a used epoch never reconnects.
const REMEMBERED_EPOCHS: usize = 1024;

pub struct Subscription {
    pub id: String,
    pub view_epoch: String,
    pub incarnation: Uuid,
    pub core_generation: String,
    pub store_generation: String,
    pub link_id: u64,
    pub stream: Mutex<StreamSender<tauri::ipc::Channel<UiFrame>>>,
}

pub struct Bridge {
    pub app_identifier: String,
    pub agent: Option<AgentPaths>,
    pub views: ViewRegistry,
    link: Mutex<Option<Arc<CompanionLink>>>,
    subscriptions: Mutex<HashMap<String, Arc<Subscription>>>,
    used_epochs: Mutex<(HashSet<String>, VecDeque<String>)>,
    queries_in_flight: AtomicUsize,
    large_replies: Mutex<HashMap<Uuid, Instant>>,
    recovery: Mutex<Option<RecoveryHook>>,
    event_log: Mutex<Option<EventLog>>,
}

pub fn link_error(error: LinkError) -> UiError {
    match error {
        LinkError::Closed => UiError::new(
            UiErrorCode::CompanionUnavailable,
            "companion connection closed",
        ),
        LinkError::Timeout => UiError::new(
            UiErrorCode::CompanionUnavailable,
            "companion did not answer in time",
        ),
        LinkError::Rejected(error) => {
            let code = match error.code {
                ControlErrorCode::Conflict => UiErrorCode::Conflict,
                ControlErrorCode::NotFound | ControlErrorCode::BadRequest => {
                    UiErrorCode::InvalidRequest
                }
                ControlErrorCode::Busy => UiErrorCode::TooManyInFlight,
                ControlErrorCode::UnknownSubscription => UiErrorCode::UnknownSubscription,
                _ => UiErrorCode::CompanionRejected,
            };
            UiError::new(code, error.detail)
        }
    }
}

fn stream_error(error: &StreamError) -> UiError {
    match error {
        StreamError::SnapshotExceedsBound { bytes, frames } => UiError::new(
            UiErrorCode::SnapshotExceedsBound,
            format!("snapshot of {bytes} bytes / {frames} frames exceeds the bound"),
        ),
        other => UiError::new(UiErrorCode::Internal, format!("stream: {other:?}")),
    }
}

/// Releases a query slot when dropped.
pub struct QuerySlot<'a>(&'a AtomicUsize);

impl Drop for QuerySlot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Bridge {
    pub fn new(app_identifier: String) -> Self {
        let agent_identifier = threadspace_relay::paths::agent_identifier_for(&app_identifier);
        Self {
            agent: AgentPaths::for_agent(&agent_identifier),
            app_identifier,
            views: ViewRegistry::default(),
            link: Mutex::new(None),
            subscriptions: Mutex::new(HashMap::new()),
            used_epochs: Mutex::new((HashSet::new(), VecDeque::new())),
            queries_in_flight: AtomicUsize::new(0),
            large_replies: Mutex::new(HashMap::new()),
            recovery: Mutex::new(None),
            event_log: Mutex::new(None),
        }
    }

    pub fn set_event_log(&self, log: EventLog) {
        if let Ok(mut slot) = self.event_log.lock() {
            *slot = Some(log);
        }
    }

    fn log(&self, event: &str, detail: serde_json::Value) {
        if let Ok(slot) = self.event_log.lock()
            && let Some(log) = slot.as_ref()
        {
            log(event, detail);
        }
    }

    pub fn set_recovery_hook(&self, hook: RecoveryHook) {
        if let Ok(mut slot) = self.recovery.lock() {
            *slot = Some(hook);
        }
    }

    /// Records a reply sent to `incarnation`; large ones may sit in the
    /// framework cache until the page fetches them.
    pub fn note_reply(&self, incarnation: Uuid, bytes: usize) {
        if bytes >= LARGE_REPLY_BYTES
            && let Ok(mut map) = self.large_replies.lock()
        {
            map.insert(incarnation, Instant::now());
        }
    }

    fn recent_large_reply(&self, incarnation: Uuid) -> bool {
        self.large_replies
            .lock()
            .ok()
            .and_then(|map| map.get(&incarnation).copied())
            .is_some_and(|at| at.elapsed() < LARGE_REPLY_WINDOW)
    }

    fn request_recovery(&self, incarnation: Uuid, reason: &'static str) {
        if !self.views.is_active(incarnation) {
            return;
        }
        if let Ok(slot) = self.recovery.lock()
            && let Some(hook) = slot.as_ref()
        {
            hook(incarnation, reason);
        }
    }

    pub fn agent_identifier(&self) -> String {
        threadspace_relay::paths::agent_identifier_for(&self.app_identifier)
    }

    /// The current companion link, connecting on demand.
    pub fn link(self: &Arc<Self>) -> Result<Arc<CompanionLink>, UiError> {
        let mut slot = self
            .link
            .lock()
            .map_err(|_| UiError::new(UiErrorCode::Internal, "link lock"))?;
        if let Some(link) = slot.as_ref().filter(|link| link.is_alive()) {
            return Ok(Arc::clone(link));
        }
        let paths = self
            .agent
            .as_ref()
            .ok_or_else(|| UiError::new(UiErrorCode::Internal, "invalid agent identifier"))?;
        let on_push: Weak<Self> = Arc::downgrade(self);
        let on_closed: Weak<Self> = Arc::downgrade(self);
        let link = CompanionLink::open(
            &paths.locator,
            move |link_id, message| {
                if let Some(bridge) = on_push.upgrade() {
                    bridge.on_push(link_id, message);
                }
            },
            move |link_id| {
                if let Some(bridge) = on_closed.upgrade() {
                    bridge.on_link_closed(link_id);
                }
            },
        )
        .map_err(|error| UiError::new(UiErrorCode::CompanionUnavailable, error.to_string()))?;
        *slot = Some(Arc::clone(&link));
        Ok(link)
    }

    pub fn current_link(&self) -> Option<Arc<CompanionLink>> {
        self.link
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().filter(|link| link.is_alive()).cloned())
    }

    pub fn subscription_count(&self) -> usize {
        self.subscriptions.lock().map(|map| map.len()).unwrap_or(0)
    }

    fn remember_epoch(&self, epoch: &str) -> bool {
        let Ok(mut guard) = self.used_epochs.lock() else {
            return false;
        };
        let (set, order) = &mut *guard;
        if !set.insert(epoch.to_owned()) {
            return false;
        }
        order.push_back(epoch.to_owned());
        if order.len() > REMEMBERED_EPOCHS
            && let Some(oldest) = order.pop_front()
        {
            set.remove(&oldest);
        }
        true
    }

    pub fn acquire_query_slot(&self) -> Result<QuerySlot<'_>, UiError> {
        if self.queries_in_flight.fetch_add(1, Ordering::AcqRel) >= QUERY_MAX_IN_FLIGHT {
            self.queries_in_flight.fetch_sub(1, Ordering::AcqRel);
            return Err(UiError::new(
                UiErrorCode::TooManyInFlight,
                "four queries already in flight",
            ));
        }
        Ok(QuerySlot(&self.queries_in_flight))
    }

    /// `ui_connect`: register the subscription, attach the view on the single
    /// writer at cursor S, then stream the bounded snapshot. Frames may reach
    /// the renderer before this returns.
    pub async fn connect_view(
        self: &Arc<Self>,
        incarnation: Uuid,
        request: UiConnectRequest,
        channel: tauri::ipc::Channel<UiFrame>,
    ) -> Result<UiConnectReply, UiError> {
        if request.protocol_version != UI_PROTOCOL_VERSION {
            return Err(UiError::new(
                UiErrorCode::UnsupportedProtocol,
                "unsupported UI protocol version",
            ));
        }
        parse_uuid(&request.view_epoch, "viewEpoch")?;
        if !self.remember_epoch(&request.view_epoch) {
            return Err(UiError::new(
                UiErrorCode::StaleContext,
                "view epoch was already used",
            ));
        }
        let link = self.link()?;
        let subscription_id = Uuid::new_v4().to_string();
        let identity = FrameIdentity {
            store_generation: link.hello.store_generation.clone(),
            core_generation: link.hello.core_generation.clone(),
            subscription_id: subscription_id.clone(),
            view_epoch: request.view_epoch.clone(),
        };
        let subscription = Arc::new(Subscription {
            id: subscription_id.clone(),
            view_epoch: request.view_epoch.clone(),
            incarnation,
            core_generation: link.hello.core_generation.clone(),
            store_generation: link.hello.store_generation.clone(),
            link_id: link.id,
            stream: Mutex::new(StreamSender::new(channel, identity)),
        });
        if let Ok(mut map) = self.subscriptions.lock() {
            map.insert(subscription_id.clone(), Arc::clone(&subscription));
        }
        let attached = link
            .request(
                ControlRequestBody::AttachView {
                    subscription_id: subscription_id.clone(),
                },
                REQUEST_TIMEOUT,
            )
            .await;
        let result = match attached {
            Ok(ControlResponseBody::ViewAttached {
                cursor, snapshot, ..
            }) => subscription
                .stream
                .lock()
                .map_err(|_| UiError::new(UiErrorCode::Internal, "stream lock"))
                .and_then(|mut stream| {
                    stream
                        .emit_snapshot(&cursor, &snapshot)
                        .map_err(|error| stream_error(&error))
                }),
            Ok(_) => Err(UiError::new(
                UiErrorCode::Internal,
                "unexpected attach reply",
            )),
            Err(error) => Err(link_error(error)),
        };
        if let Err(error) = result {
            self.retire(&subscription_id);
            return Err(error);
        }
        Ok(UiConnectReply {
            subscription_id,
            view_epoch: request.view_epoch,
            core_generation: subscription.core_generation.clone(),
            store_generation: subscription.store_generation.clone(),
            limits: StreamLimits::FROZEN,
        })
    }

    fn subscription_for(
        &self,
        incarnation: Uuid,
        subscription_id: &str,
        view_epoch: &str,
    ) -> Result<Arc<Subscription>, UiError> {
        let subscription = self
            .subscriptions
            .lock()
            .ok()
            .and_then(|map| map.get(subscription_id).cloned())
            .ok_or_else(|| {
                UiError::new(
                    UiErrorCode::UnknownSubscription,
                    "unknown or retired subscription",
                )
            })?;
        if subscription.incarnation != incarnation || subscription.view_epoch != view_epoch {
            return Err(UiError::new(
                UiErrorCode::StaleContext,
                "subscription belongs to another view",
            ));
        }
        Ok(subscription)
    }

    /// Validates a subscribed call context against the current registration.
    pub fn validate_context(
        &self,
        incarnation: Uuid,
        context: &UiCallContext,
    ) -> Result<Arc<Subscription>, UiError> {
        let subscription =
            self.subscription_for(incarnation, &context.subscription_id, &context.view_epoch)?;
        if subscription.core_generation != context.core_generation
            || subscription.store_generation != context.store_generation
        {
            return Err(UiError::new(
                UiErrorCode::StaleContext,
                "companion generation changed",
            ));
        }
        Ok(subscription)
    }

    pub fn ack(
        self: &Arc<Self>,
        incarnation: Uuid,
        request: &UiAckRequest,
    ) -> Result<UiAckReply, UiError> {
        if parse_cursor(&request.applied_journal_cursor).is_none() {
            return Err(UiError::invalid(
                "appliedJournalCursor must be a canonical cursor",
            ));
        }
        let subscription =
            self.subscription_for(incarnation, &request.subscription_id, &request.view_epoch)?;
        let outcome = subscription
            .stream
            .lock()
            .map_err(|_| UiError::new(UiErrorCode::Internal, "stream lock"))?
            .ack(
                request.highest_applied_stream_seq,
                &request.applied_journal_cursor,
            )
            .map_err(|error| match error {
                AckError::Retired => {
                    UiError::new(UiErrorCode::UnknownSubscription, "subscription retired")
                }
                other => UiError::new(UiErrorCode::AckRejected, format!("{other:?}")),
            })?;
        let notify: Vec<ControlRequestBody> = outcome
            .newly_hydrated
            .then(|| ControlRequestBody::ViewHydrated {
                subscription_id: subscription.id.clone(),
            })
            .into_iter()
            .chain(outcome.consumed_intents.iter().map(|intent_id| {
                ControlRequestBody::IntentConsumed {
                    intent_id: intent_id.clone(),
                }
            }))
            .collect();
        if !notify.is_empty()
            && let Some(link) = self
                .current_link()
                .filter(|link| link.id == subscription.link_id)
        {
            tauri::async_runtime::spawn(async move {
                for body in notify {
                    let _ = link.request(body, REQUEST_TIMEOUT).await;
                }
            });
        }
        Ok(UiAckReply {
            acknowledged_through: outcome.acknowledged_through,
            hydrated: outcome.hydrated,
        })
    }

    pub fn disconnect(
        self: &Arc<Self>,
        incarnation: Uuid,
        request: &UiDisconnectRequest,
    ) -> Result<(), UiError> {
        let subscription =
            self.subscription_for(incarnation, &request.subscription_id, &request.view_epoch)?;
        self.retire(&subscription.id);
        Ok(())
    }

    /// Retires one subscription and best-effort detaches it from the companion.
    pub fn retire(&self, subscription_id: &str) {
        let removed = self
            .subscriptions
            .lock()
            .ok()
            .and_then(|mut map| map.remove(subscription_id));
        let Some(subscription) = removed else { return };
        let cached = subscription
            .stream
            .lock()
            .map(|mut stream| {
                let cached = stream.may_hold_cached_frames();
                stream.retire();
                cached
            })
            .unwrap_or(false);
        // Dropping a Channel or reloading the page does not purge the
        // framework's per-view cache; only removing the actual view does.
        if cached || self.recent_large_reply(subscription.incarnation) {
            self.request_recovery(subscription.incarnation, "UNCONSUMED_DATA_ON_RETIREMENT");
        }
        if let Some(link) = self
            .current_link()
            .filter(|link| link.id == subscription.link_id)
        {
            let subscription_id = subscription.id.clone();
            tauri::async_runtime::spawn(async move {
                let _ = link
                    .request(
                        ControlRequestBody::DetachView { subscription_id },
                        REQUEST_TIMEOUT,
                    )
                    .await;
            });
        }
    }

    /// Native page load/close: retire every subscription of that incarnation
    /// even if the renderer never unsubscribed (SPEC §18.5).
    pub fn retire_incarnation(&self, incarnation: Uuid) {
        let ids: Vec<String> = self
            .subscriptions
            .lock()
            .map(|map| {
                map.values()
                    .filter(|sub| sub.incarnation == incarnation)
                    .map(|sub| sub.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        for id in ids {
            self.retire(&id);
        }
    }

    fn on_push(&self, link_id: u64, message: ControlMessage) {
        let (subscription_id, outcome) = match message {
            ControlMessage::ViewPatch {
                subscription_id,
                cursor,
                patch,
            } => {
                let subscription = self
                    .subscriptions
                    .lock()
                    .ok()
                    .and_then(|map| map.get(&subscription_id).cloned());
                let outcome = subscription
                    .filter(|sub| sub.link_id == link_id)
                    .map(|sub| {
                        sub.stream
                            .lock()
                            .map_err(|_| StreamError::Retired)
                            .and_then(|mut stream| stream.push_patch(&cursor, patch))
                    });
                (subscription_id, outcome)
            }
            ControlMessage::Intent {
                subscription_id,
                cursor,
                intent,
            } => {
                let subscription = self
                    .subscriptions
                    .lock()
                    .ok()
                    .and_then(|map| map.get(&subscription_id).cloned());
                let outcome = subscription
                    .filter(|sub| sub.link_id == link_id)
                    .map(|sub| {
                        sub.stream
                            .lock()
                            .map_err(|_| StreamError::Retired)
                            .and_then(|mut stream| stream.push_intent(&cursor, intent))
                    });
                (subscription_id, outcome)
            }
            ControlMessage::Response { .. } => return,
        };
        if let Some(Err(error)) = outcome {
            self.log(
                "SUBSCRIPTION_RETIRED",
                serde_json::json!({ "subscriptionId": subscription_id, "reason": format!("{error:?}") }),
            );
            self.retire(&subscription_id);
        }
    }

    fn on_link_closed(&self, link_id: u64) {
        let ids: Vec<String> = self
            .subscriptions
            .lock()
            .map(|map| {
                map.values()
                    .filter(|sub| sub.link_id == link_id)
                    .map(|sub| sub.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        for id in ids {
            self.retire(&id);
        }
        if let Ok(mut slot) = self.link.lock()
            && slot.as_ref().is_some_and(|link| link.id == link_id)
        {
            *slot = None;
        }
    }

    /// Sends a heartbeat on every hydrated subscription; a renderer that stops
    /// acknowledging exhausts its window and is retired.
    pub fn heartbeat_tick(&self) {
        let subscriptions: Vec<Arc<Subscription>> = self
            .subscriptions
            .lock()
            .map(|map| map.values().cloned().collect())
            .unwrap_or_default();
        for subscription in subscriptions {
            let failure = subscription
                .stream
                .lock()
                .map(|mut stream| stream.heartbeat().err())
                .unwrap_or(Some(StreamError::Retired));
            if let Some(error) = failure {
                self.log(
                    "SUBSCRIPTION_RETIRED",
                    serde_json::json!({ "subscriptionId": subscription.id, "reason": format!("heartbeat: {error:?}") }),
                );
                self.retire(&subscription.id);
            }
        }
    }
}
