//! Forwarder mode for an instance that lost the writer lock (SPEC §7.5). A
//! notification response can launch a second companion while the incumbent
//! runs or restarts; that instance never writes. It forwards the response to
//! the verified incumbent over the control socket and exits, or exits after a
//! bounded wait when nothing arrives.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use serde_json::json;
use threadspace_contracts::control::{ClientRole, ControlRequestBody};
use threadspace_relay::client::connect;

use crate::log;
use crate::EXIT_WRITER_LOCK_HELD;

/// Long enough for a launch-time notification response to arrive.
const FORWARD_WINDOW: Duration = Duration::from_secs(15);

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
