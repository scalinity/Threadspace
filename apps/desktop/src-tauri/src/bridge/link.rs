//! The desktop's verified connection to the companion's control socket. One
//! reader thread routes responses to waiting requests and hands pushed
//! patches/intents to the bridge. A lost link fails every waiter; the bridge
//! reconnects on the next operation that needs the companion.

use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use threadspace_contracts::control::{
    ClientRole, ControlError, ControlMessage, ControlOutcome, ControlRequest, ControlRequestBody, ControlResponseBody,
};
use threadspace_contracts::limits::CONTROL_FRAME_MAX_BYTES;
use threadspace_relay::client::{self, ClientError, HelloInfo};
use threadspace_relay::frame::{read_frame, write_frame};
use tokio::sync::oneshot;

static NEXT_LINK: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub enum LinkError {
    Closed,
    Timeout,
    Rejected(ControlError),
}

pub struct CompanionLink {
    pub id: u64,
    pub hello: HelloInfo,
    writer: Mutex<UnixStream>,
    pending: Mutex<HashMap<u64, oneshot::Sender<ControlOutcome>>>,
    next_request: AtomicU64,
    alive: AtomicBool,
}

impl CompanionLink {
    pub fn open(
        locator: &Path,
        on_push: impl Fn(u64, ControlMessage) + Send + 'static,
        on_closed: impl FnOnce(u64) + Send + 'static,
    ) -> Result<Arc<Self>, ClientError> {
        let connection = client::connect(locator, ClientRole::Ui, Duration::from_secs(2))?;
        let reader = connection.stream.try_clone().map_err(ClientError::Connect)?;
        reader.set_read_timeout(None).map_err(ClientError::Connect)?;
        let link = Arc::new(Self {
            id: NEXT_LINK.fetch_add(1, Ordering::Relaxed),
            hello: connection.hello,
            writer: Mutex::new(connection.stream),
            pending: Mutex::new(HashMap::new()),
            next_request: AtomicU64::new(1),
            alive: AtomicBool::new(true),
        });
        let reader_link = Arc::clone(&link);
        thread::Builder::new()
            .name("companion-link".into())
            .spawn(move || {
                let mut reader = reader;
                while let Ok(message) = read_frame::<_, ControlMessage>(&mut reader, CONTROL_FRAME_MAX_BYTES) {
                    match message {
                        ControlMessage::Response { request_id, outcome } => {
                            let waiter = reader_link.pending.lock().ok().and_then(|mut map| map.remove(&request_id));
                            if let Some(waiter) = waiter {
                                let _ = waiter.send(outcome);
                            }
                        }
                        push => on_push(reader_link.id, push),
                    }
                }
                reader_link.alive.store(false, Ordering::Release);
                if let Ok(mut map) = reader_link.pending.lock() {
                    map.clear();
                }
                on_closed(reader_link.id);
            })
            .map_err(ClientError::Connect)?;
        Ok(link)
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    pub async fn request(&self, body: ControlRequestBody, timeout: Duration) -> Result<ControlResponseBody, LinkError> {
        if !self.is_alive() {
            return Err(LinkError::Closed);
        }
        let request_id = self.next_request.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().map_err(|_| LinkError::Closed)?.insert(request_id, sender);
        let written = self
            .writer
            .lock()
            .map_err(|_| LinkError::Closed)
            .and_then(|mut stream| {
                write_frame(&mut *stream, &ControlRequest { request_id, body }, CONTROL_FRAME_MAX_BYTES)
                    .map_err(|_| LinkError::Closed)
            });
        if let Err(error) = written {
            if let Ok(mut map) = self.pending.lock() {
                map.remove(&request_id);
            }
            return Err(error);
        }
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(ControlOutcome::Ok(body))) => Ok(*body),
            Ok(Ok(ControlOutcome::Err(error))) => Err(LinkError::Rejected(error)),
            Ok(Err(_)) => Err(LinkError::Closed),
            Err(_) => {
                if let Ok(mut map) = self.pending.lock() {
                    map.remove(&request_id);
                }
                Err(LinkError::Timeout)
            }
        }
    }
}
