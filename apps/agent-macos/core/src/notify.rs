//! Notification outbox worker (SPEC §7.5–7.6). Runs after the attention
//! transaction commits: it reads the actual system settings for the
//! companion's identity, submits through `UNUserNotificationCenter`, and
//! journals the outcome. Denial leaves the durable attention item intact.

use std::sync::mpsc::{Receiver, SyncSender};
use std::thread;
use std::time::Duration;

use serde_json::json;
use threadspace_contracts::projection::NotificationState;
use threadspace_journal::NotificationIntent;

use crate::bridge::{self, BridgeEvent, BridgeRequest};
use crate::log;
use crate::writer::WriterCommand;

fn authorized(status: &str) -> bool {
    matches!(status, "authorized" | "provisional" | "ephemeral")
}

fn submit(intent: &NotificationIntent) -> (NotificationState, String) {
    let Some(settings) = bridge::notification_settings(Duration::from_secs(5)) else {
        return (
            NotificationState::Uncertain,
            "notification settings unavailable".into(),
        );
    };
    if !authorized(&settings.authorization_status) {
        return (
            NotificationState::Failed,
            format!(
                "not authorized ({}); attention retained",
                settings.authorization_status
            ),
        );
    }
    let posted = bridge::call(
        |correlation_id| BridgeRequest::PostNotification {
            correlation_id,
            request_id: intent.request_id.clone(),
            title: intent.title.clone(),
            body: intent.body.clone(),
            attention_id: intent.attention_id.clone(),
            session_id: intent.session_id.clone(),
        },
        Duration::from_secs(10),
    );
    match posted {
        Ok(BridgeEvent::NotificationPosted { error: None, .. }) => (
            NotificationState::Submitted,
            format!(
                "accepted by UNUserNotificationCenter (alert {})",
                settings.alert_setting
            ),
        ),
        Ok(BridgeEvent::NotificationPosted {
            error: Some(error), ..
        }) => (NotificationState::Failed, error),
        Ok(_) => (
            NotificationState::Uncertain,
            "unexpected bridge answer".into(),
        ),
        // A timeout is uncertainty, never assumed delivery.
        Err(error) => (NotificationState::Uncertain, error.to_string()),
    }
}

pub fn spawn(
    intents: Receiver<NotificationIntent>,
    writer: SyncSender<WriterCommand>,
) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new().name("notification-outbox".into()).spawn(move || {
        for intent in intents {
            let (state, detail) = submit(&intent);
            log::info(
                "NOTIFICATION_SUBMISSION",
                json!({ "requestId": intent.request_id, "attentionId": intent.attention_id, "state": state, "detail": detail }),
            );
            if writer
                .send(WriterCommand::NotificationOutcome { request_id: intent.request_id, state, detail })
                .is_err()
            {
                break;
            }
        }
    })
}
