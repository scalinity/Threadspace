//! Forwarder mode for an instance that lost the writer lock (SPEC §7.5). A
//! notification response can launch a second companion while the incumbent
//! runs or restarts; that instance never writes. It forwards the response to
//! the verified incumbent over the control socket and exits, or exits after a
//! bounded wait when nothing arrives.
//!
//! The login item's own companion first tries to claim the store: an
//! unsupervised incumbent yields it and exits once its writer has finished
//! what it accepted; its pending intents are already in the store
//! (SPEC §18.9). A forwarded response is recorded first, so a failed hand-off
//! leaves it for the next writer.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;
use threadspace_contracts::control::{ClientRole, ControlRequestBody, ControlResponseBody};
use threadspace_journal::{LockError, WriterLock};
use threadspace_relay::client::{BlockingClient, ClientError, connect};

use crate::EXIT_WRITER_LOCK_HELD;
use crate::intent_store;
use crate::log;

/// Long enough for a launch-time notification response to arrive.
const FORWARD_WINDOW: Duration = Duration::from_secs(15);
/// How long a claimant waits for a yielding incumbent to release the lock.
const CLAIM_WAIT: Duration = Duration::from_secs(10);

/// Asks the incumbent to yield the store, then takes the writer lock it
/// releases. The pending intents travel through the store, not the reply,
/// so a lost reply only means waiting for the lock. `None` when the
/// incumbent refuses (it is the supervised companion) or the lock does not
/// come free in time; the caller then forwards and exits nonzero, so launchd
/// tries again.
pub fn claim(locator: &Path, store_dir: &Path) -> Option<WriterLock> {
    let reply = connect(locator, ClientRole::Ui, Duration::from_secs(2)).and_then(|connection| {
        BlockingClient::new(connection).request(ControlRequestBody::YieldWriter)
    });
    let reported = match reply {
        Ok(ControlResponseBody::WriterYielded { pending_intent_ids }) => {
            json!(pending_intent_ids)
        }
        Err(ClientError::Rejected(error)) => {
            log::info(
                "WRITER_CLAIM_REFUSED",
                json!({ "code": error.code, "detail": error.detail }),
            );
            return None;
        }
        other => {
            let detail: String = format!("{other:?}").chars().take(200).collect();
            log::info("WRITER_CLAIM_UNANSWERED", json!({ "reply": detail }));
            serde_json::Value::Null
        }
    };
    let deadline = Instant::now() + CLAIM_WAIT;
    loop {
        match WriterLock::acquire(store_dir) {
            Ok(lock) => {
                log::info(
                    "WRITER_CLAIMED",
                    json!({ "reportedPendingIntents": reported }),
                );
                return Some(lock);
            }
            Err(LockError::Held { .. }) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => {
                log::warn(
                    "WRITER_CLAIM_LOCK_FAILED",
                    json!({ "error": error.to_string() }),
                );
                return None;
            }
        }
    }
}

/// The incumbent's locator and the store a response is spooled in.
static FORWARDING: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();

pub fn active() -> bool {
    FORWARDING.get().is_some()
}

pub fn enter(locator: PathBuf, store_dir: PathBuf) {
    if FORWARDING.set((locator, store_dir)).is_err() {
        return;
    }
    let _ = thread::Builder::new()
        .name("forwarder-exit".into())
        .spawn(|| {
            thread::sleep(FORWARD_WINDOW);
            log::warn("FORWARDER_EXIT", json!({ "forwarded": false }));
            std::process::exit(EXIT_WRITER_LOCK_HELD);
        });
}

/// Forwards a response this instance received at `received`, with how long
/// ago that was, so the incumbent's Return keeps the original budget.
pub fn notification_response(request_id: &str, attention_id: &str, received: Instant) {
    let Some((locator, store_dir)) = FORWARDING.get() else {
        return;
    };
    intent_store::record_for_forwarding(store_dir, request_id, attention_id);
    let outcome = connect(locator, ClientRole::Ui, Duration::from_secs(2))
        .map_err(|error| error.to_string())
        .and_then(|connection| {
            let mut client = threadspace_relay::client::BlockingClient::new(connection);
            client
                .request(ControlRequestBody::ForwardNotificationResponse {
                    notification_request_id: request_id.to_owned(),
                    attention_id: attention_id.to_owned(),
                    received_ago_ms: Some(received.elapsed().as_millis() as u64),
                })
                .map_err(|error| error.to_string())
        });
    log::info(
        "NOTIFICATION_RESPONSE_FORWARDED",
        json!({ "requestId": request_id, "attentionId": attention_id, "ok": outcome.is_ok(), "error": outcome.err() }),
    );
    // Never a successful exit: when this instance is the login item's, launchd
    // relaunches it until it holds the writer lock (SPEC §18.9).
    std::process::exit(EXIT_WRITER_LOCK_HELD);
}
