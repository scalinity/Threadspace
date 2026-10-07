//! Qualification only (absent from release builds): a companion core on a
//! disposable store, without the AppKit shell. It runs the real single
//! writer, the capture event socket and the spool drainer with capture
//! admission open, so the native capture benchmark and relay checks run the
//! production capture path end to end without touching an installed
//! identity's store. Notifications, discovery and responses are not started.
//! One per process: the runtime gate is process-global.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, SyncSender};
use std::thread;

use threadspace_contracts::diagnostics::LaunchProvenance;
use threadspace_journal::{Journal, WriterLock};
use threadspace_relay::locator::{self, LOCATOR_SCHEMA, RuntimeLocator};
use threadspace_relay::runtime::{bind_private_socket, create_runtime_dir};
use threadspace_relay::spool::Spool;
use uuid::Uuid;

use crate::state::RUNTIME;
use crate::writer::{self, WriterCommand, WriterSetup};
use crate::{events, intent_store, log, own_identity, private_dir, spool_drain};

pub struct FixtureCompanion {
    pub store_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub events_socket: PathBuf,
    pub locator: PathBuf,
    writer: SyncSender<WriterCommand>,
    _lock: WriterLock,
}

impl FixtureCompanion {
    /// One spool drain pass through the writer; the records committed.
    pub fn drain_spool(&self) -> usize {
        spool_drain::drain(&Spool::at(&self.store_dir), &self.writer)
    }

    /// Opens or closes capture admission (the owner's observation preference).
    pub fn set_admission(&self, open: bool) {
        RUNTIME.set_observation_enabled(open);
    }
}

fn discard<T: Send + 'static>(receiver: mpsc::Receiver<T>) {
    let _ = thread::Builder::new()
        .name("fixture-discard".into())
        .spawn(move || for _ in receiver {});
}

pub fn start(store_dir: &Path) -> Result<FixtureCompanion, String> {
    private_dir(store_dir).map_err(|e| e.to_string())?;
    log::init(&store_dir.join("logs"));
    RUNTIME.set_provenance(LaunchProvenance::LoginItem);
    RUNTIME.set_observation_enabled(true);
    let lock = WriterLock::acquire(store_dir).map_err(|e| e.to_string())?;
    let core_generation = Uuid::new_v4().to_string();
    let journal = Journal::open(&store_dir.join("journal.sqlite3"), &core_generation, log::now_ms())
        .map_err(|e| e.to_string())?;
    let store_generation = journal.store_generation().to_owned();
    let identity = own_identity()?;
    let (writer_tx, writer_rx) = mpsc::sync_channel(256);
    let (notify_tx, notify_rx) = mpsc::sync_channel(64);
    let (respond_tx, respond_rx) = mpsc::sync_channel(16);
    discard(notify_rx);
    discard(respond_rx);
    let (backlog, found) = intent_store::load_backlog(store_dir);
    writer::spawn(WriterSetup {
        journal,
        commands: writer_rx,
        sender: writer_tx.clone(),
        notifier: notify_tx,
        responder: respond_tx,
        identity: identity.clone(),
        store_dir: store_dir.to_path_buf(),
        backlog,
        backlog_unavailable: matches!(found, intent_store::Found::Unreadable { .. }),
    })
    .map_err(|e| e.to_string())?;
    let runtime_dir = create_runtime_dir().map_err(|e| e.to_string())?;
    let (listener, events_socket) =
        bind_private_socket(&runtime_dir, "events.sock").map_err(|e| e.to_string())?;
    events::spawn(listener, writer_tx.clone()).map_err(|e| e.to_string())?;
    let locator_path = store_dir.join("runtime-locator.json");
    locator::write_atomic(
        &locator_path,
        &RuntimeLocator {
            schema: LOCATOR_SCHEMA,
            bundle_identifier: "fixture".into(),
            runtime_dir: runtime_dir.display().to_string(),
            control_socket: String::new(),
            events_socket: Some(events_socket.display().to_string()),
            core_generation,
            store_generation,
            companion: identity,
            written_at_ms: log::now_ms(),
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(FixtureCompanion {
        store_dir: store_dir.to_path_buf(),
        runtime_dir,
        events_socket,
        locator: locator_path,
        writer: writer_tx,
        _lock: lock,
    })
}
