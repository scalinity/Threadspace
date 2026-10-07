//! Accepted notification intents outlive the process that accepted them
//! (SPEC §7.5, §18.9). The writer persists every pending notification or
//! inspector intent before it delivers it and removes it once a view has
//! consumed it; whichever companion next holds the writer lock loads them.
//!
//! A response that reaches this instance while no writer of its own can take
//! it — the writer has stopped accepting before an exit, its queue is full,
//! or this instance only forwards — is spooled in the store for the next
//! writer instead of being dropped. Every voluntary exit of a writer seals
//! acceptance first, so a response being spooled is never cut off by it.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use serde_json::json;
use threadspace_contracts::projection::{IntentAction, NativeIntent};
use uuid::Uuid;

use crate::log;
use crate::writer::WriterCommand;

const PENDING: &str = "pending-intents.json";
const SPOOL: &str = "responses";

/// Whether this process's writer still takes responses. Held while one is
/// handed over or spooled, so `seal` waits for any in flight.
static ACCEPTING: Mutex<bool> = Mutex::new(true);

/// A notification response kept for the next writer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spooled {
    pub notification_request_id: String,
    pub attention_id: String,
    pub spooled_at_ms: i64,
}

fn valid_uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|parsed| parsed.hyphenated().to_string() == value)
}

/// Writes `bytes` to `path` so that a reader sees the old or the new file,
/// never a partial one, and the change survives a crash.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let temporary = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        std::process::id()
    ));
    let mut file = File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    File::open(dir)?.sync_all()
}

/// Intents that belong to the owner: notification, navigation and inspector
/// intents. Qualification commands stay with the process that received them.
pub fn durable(intent: &NativeIntent) -> bool {
    matches!(intent.action, IntentAction::OpenAttention { .. })
}

/// Persists the pending durable intents, replacing the previous set.
pub fn save<'a>(
    store_dir: &Path,
    intents: impl Iterator<Item = &'a NativeIntent>,
) -> std::io::Result<()> {
    let pending: Vec<&NativeIntent> = intents.filter(|intent| durable(intent)).collect();
    let bytes = serde_json::to_vec(&pending).map_err(std::io::Error::other)?;
    write_atomic(&store_dir.join(PENDING), &bytes)
}

/// The pending intents a previous writer left, in their original order.
pub fn load(store_dir: &Path) -> Vec<NativeIntent> {
    let path = store_dir.join(PENDING);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            log::error(
                "PENDING_INTENTS_UNREADABLE",
                json!({ "error": error.to_string() }),
            );
            return Vec::new();
        }
    };
    match serde_json::from_slice::<Vec<NativeIntent>>(&bytes) {
        Ok(intents) => intents
            .into_iter()
            .filter(|intent| durable(intent) && valid_uuid(&intent.intent_id))
            .collect(),
        Err(error) => {
            log::error(
                "PENDING_INTENTS_UNREADABLE",
                json!({ "error": error.to_string() }),
            );
            Vec::new()
        }
    }
}

fn spool_path(store_dir: &Path, notification_request_id: &str) -> PathBuf {
    store_dir
        .join(SPOOL)
        .join(format!("{notification_request_id}.json"))
}

/// Keeps one response for the next writer. Named by its notification
/// request, so spooling the same response twice keeps one copy.
pub fn spool(
    store_dir: &Path,
    notification_request_id: &str,
    attention_id: &str,
) -> std::io::Result<()> {
    if !valid_uuid(notification_request_id) {
        return Err(std::io::Error::other(
            "notification request ID is not a UUID",
        ));
    }
    fs::create_dir_all(store_dir.join(SPOOL))?;
    let record = Spooled {
        notification_request_id: notification_request_id.to_owned(),
        attention_id: attention_id.to_owned(),
        spooled_at_ms: log::now_ms(),
    };
    let bytes = serde_json::to_vec(&record).map_err(std::io::Error::other)?;
    write_atomic(&spool_path(store_dir, notification_request_id), &bytes)
}

/// Responses spooled for this writer, oldest first.
pub fn spooled(store_dir: &Path) -> Vec<Spooled> {
    let Ok(entries) = fs::read_dir(store_dir.join(SPOOL)) else {
        return Vec::new();
    };
    let mut records: Vec<Spooled> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_suffix(".json"))
                .is_some_and(valid_uuid)
        })
        .filter_map(|entry| fs::read(entry.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<Spooled>(&bytes).ok())
        .filter(|record| valid_uuid(&record.notification_request_id))
        .collect();
    records.sort_by_key(|record| record.spooled_at_ms);
    records
}

/// Removes a spooled response once its intent is held by the writer.
pub fn unspool(store_dir: &Path, notification_request_id: &str) {
    if !valid_uuid(notification_request_id) {
        return;
    }
    match fs::remove_file(spool_path(store_dir, notification_request_id)) {
        Ok(()) => log::info(
            "RESPONSE_UNSPOOLED",
            json!({ "requestId": notification_request_id }),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn(
            "RESPONSE_UNSPOOL_FAILED",
            json!({ "requestId": notification_request_id, "error": error.to_string() }),
        ),
    }
}

fn spool_logged(store_dir: &Path, notification_request_id: &str, attention_id: &str, why: &str) {
    match spool(store_dir, notification_request_id, attention_id) {
        Ok(()) => log::info(
            "RESPONSE_SPOOLED",
            json!({ "requestId": notification_request_id, "attentionId": attention_id, "reason": why }),
        ),
        Err(error) => log::error(
            "RESPONSE_SPOOL_FAILED",
            json!({ "requestId": notification_request_id, "reason": why, "error": error.to_string() }),
        ),
    }
}

/// Accepts one notification response: the writer takes it while it accepts;
/// otherwise it is spooled for the next writer.
pub fn accept(
    store_dir: &Path,
    writer: &SyncSender<WriterCommand>,
    notification_request_id: String,
    attention_id: String,
) {
    let accepting = ACCEPTING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !*accepting {
        spool_logged(
            store_dir,
            &notification_request_id,
            &attention_id,
            "WRITER_RELEASING",
        );
        return;
    }
    match writer.try_send(WriterCommand::NotificationResponse {
        notification_request_id,
        attention_id,
    }) {
        Ok(()) => {}
        Err(TrySendError::Full(command) | TrySendError::Disconnected(command)) => {
            if let WriterCommand::NotificationResponse {
                notification_request_id,
                attention_id,
            } = command
            {
                spool_logged(
                    store_dir,
                    &notification_request_id,
                    &attention_id,
                    "WRITER_BUSY",
                );
            }
        }
    }
}

/// A forwarding instance keeps the response before handing it on, so a
/// failed hand-off leaves it for the next writer; the writer that takes it
/// removes it.
pub fn spool_for_forwarding(store_dir: &Path, notification_request_id: &str, attention_id: &str) {
    spool_logged(
        store_dir,
        notification_request_id,
        attention_id,
        "FORWARDED",
    );
}

/// The writer stops taking responses; later ones are spooled.
pub fn close() {
    *ACCEPTING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = false;
}

/// Waits for any response being handed over or spooled and holds acceptance
/// shut until the process exits.
pub fn seal() -> MutexGuard<'static, bool> {
    ACCEPTING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use threadspace_contracts::projection::IntentSource;

    fn intent(id: &str) -> NativeIntent {
        NativeIntent {
            intent_id: id.to_owned(),
            action: IntentAction::OpenAttention {
                attention_id: Uuid::new_v4().to_string(),
                session_id: Uuid::new_v4().to_string(),
                outstanding: true,
                source: IntentSource::NotificationResponse,
                route: None,
                observation_enabled: false,
            },
        }
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ts-intents-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn pending_intents_round_trip_in_order_and_replace() {
        let dir = temp_dir();
        assert!(load(&dir).is_empty());
        let (a, b) = (Uuid::new_v4().to_string(), Uuid::new_v4().to_string());
        save(&dir, [intent(&a), intent(&b)].iter()).expect("save");
        let loaded: Vec<String> = load(&dir).into_iter().map(|i| i.intent_id).collect();
        assert_eq!(loaded, [a.clone(), b]);
        save(&dir, [intent(&a)].iter()).expect("save");
        assert_eq!(load(&dir).len(), 1);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn spooled_responses_are_kept_once_and_removed() {
        let dir = temp_dir();
        let request = Uuid::new_v4().to_string();
        let attention = Uuid::new_v4().to_string();
        spool(&dir, &request, &attention).expect("spool");
        spool(&dir, &request, &attention).expect("spool again");
        let records = spooled(&dir);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].attention_id, attention);
        unspool(&dir, &request);
        assert!(spooled(&dir).is_empty());
        assert!(spool(&dir, "../escape", &attention).is_err());
        fs::remove_dir_all(dir).ok();
    }
}
