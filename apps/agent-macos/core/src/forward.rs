//! Forwarder mode for an instance that lost the writer lock (SPEC §7.5). A
//! notification response can launch a second companion while the incumbent
//! runs or restarts; that instance never writes. It forwards the response to
//! the verified incumbent over the control socket and exits, or exits after a
//! bounded wait when nothing arrives.
//!
//! The login item's own companion first tries to claim the store: an
//! unsupervised incumbent hands over its undelivered intents and exits
//! (SPEC §18.9).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;
use threadspace_contracts::control::{ClientRole, ControlRequestBody, ControlResponseBody};
use threadspace_contracts::projection::NativeIntent;
use threadspace_journal::{LockError, WriterLock};
use threadspace_relay::client::{BlockingClient, connect};

use crate::EXIT_WRITER_LOCK_HELD;
use crate::log;

/// Long enough for a launch-time notification response to arrive.
const FORWARD_WINDOW: Duration = Duration::from_secs(15);
/// How long a claimant waits for a yielding incumbent to release the lock.
const CLAIM_WAIT: Duration = Duration::from_secs(10);

/// Asks the incumbent to yield the store, then takes the writer lock it
/// releases. `None` when the incumbent refuses (it is the supervised
/// companion), cannot be reached, or the lock does not come free in time; the
/// caller then forwards and exits nonzero, so launchd tries again.
pub fn claim(locator: &Path, store_dir: &Path) -> Option<(WriterLock, Vec<NativeIntent>)> {
    let reply = connect(locator, ClientRole::Ui, Duration::from_secs(2))
        .map_err(|error| error.to_string())
        .and_then(|connection| {
            BlockingClient::new(connection)
                .request(ControlRequestBody::YieldWriter)
                .map_err(|error| error.to_string())
        });
    let intents = match reply {
        Ok(ControlResponseBody::WriterYielded { intents }) => intents,
        other => {
            let detail: String = format!("{other:?}").chars().take(200).collect();
            log::info("WRITER_CLAIM_REFUSED", json!({ "reply": detail }));
            return None;
        }
    };
    let deadline = Instant::now() + CLAIM_WAIT;
    loop {
        match WriterLock::acquire(store_dir) {
            Ok(lock) => {
                log::info(
                    "WRITER_CLAIMED",
                    json!({ "inheritedIntents": intents.len() }),
                );
                return Some((lock, intents));
            }
            Err(LockError::Held { .. }) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => {
                log::warn(
                    "WRITER_CLAIM_LOCK_FAILED",
                    json!({ "error": error.to_string(), "droppedIntents": intents.len() }),
                );
                return None;
            }
        }
    }
}

static LOCATOR: OnceLock<PathBuf> = OnceLock::new();

pub fn active() -> bool {
    LOCATOR.get().is_some()
}

pub fn enter(locator: PathBuf) {
    if LOCATOR.set(locator).is_err() {
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

pub fn notification_response(request_id: &str, attention_id: &str) {
    let Some(locator) = LOCATOR.get() else {
        return;
    };
    let outcome = connect(locator, ClientRole::Ui, Duration::from_secs(2))
        .map_err(|error| error.to_string())
        .and_then(|connection| {
            let mut client = threadspace_relay::client::BlockingClient::new(connection);
            client
                .request(ControlRequestBody::ForwardNotificationResponse {
                    notification_request_id: request_id.to_owned(),
                    attention_id: attention_id.to_owned(),
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
