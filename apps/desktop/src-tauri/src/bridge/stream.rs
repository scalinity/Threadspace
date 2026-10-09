//! One serialized sender per subscription (SPEC §18.4): contiguous stream
//! sequence numbers, a complete bounded snapshot (≤512 KiB, ≤10 frames,
//! ≤64 KiB per frame) and a 32-frame / 2 MiB unacknowledged window. An
//! exhausted window or stalled ACKs retire the subscription; nothing is
//! buffered without bound.

use std::collections::{BTreeMap, VecDeque};

use threadspace_contracts::frames::{CHUNK_BUDGET, chunk_for_frames};
use threadspace_contracts::limits::{
    FRAME_MAX_BYTES, SNAPSHOT_MAX_BYTES, SNAPSHOT_MAX_FRAMES, WINDOW_MAX_BYTES, WINDOW_MAX_FRAMES,
};
use threadspace_contracts::projection::{FleetSnapshot, NativeIntent, ProjectionPatch};
use threadspace_contracts::ui::{FrameHeader, UI_PROTOCOL_VERSION, UiFrame, UiFrameBody};

/// The pinned Channel sends JSON payloads at or above this size through its
/// per-webview cache instead of direct evaluation (tauri 3.0.0-alpha.4
/// `MAX_JSON_DIRECT_EXECUTE_THRESHOLD`).
pub const CHANNEL_CACHE_THRESHOLD: usize = 8192;
/// Changes that may arrive between `AttachView` and snapshot emission.
const MAX_BUFFERED_BEFORE_SNAPSHOT: usize = 32;

/// Where frames go. Implemented by the Tauri Channel and by test sinks.
pub trait FrameSink: Send {
    fn deliver(&self, frame: UiFrame) -> Result<(), String>;
}

impl FrameSink for tauri::ipc::Channel<UiFrame> {
    fn deliver(&self, frame: UiFrame) -> Result<(), String> {
        self.send(frame).map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamError {
    Retired,
    FrameTooLarge(usize),
    WindowExhausted,
    SnapshotExceedsBound { bytes: usize, frames: usize },
    ChannelClosed(String),
    BufferOverflow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AckError {
    Retired,
    NotMonotonic { acked_through: u32 },
    FutureSequence { last_sent: u32 },
    CursorMismatch { expected: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AckOutcome {
    pub acknowledged_through: u32,
    pub hydrated: bool,
    pub newly_hydrated: bool,
    pub consumed_intents: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FrameIdentity {
    pub store_generation: String,
    pub core_generation: String,
    pub subscription_id: String,
    pub view_epoch: String,
}

#[derive(Debug)]
struct Sent {
    seq: u32,
    bytes: usize,
    cursor: String,
}

enum Pending {
    Patch {
        cursor: String,
        patch: Box<ProjectionPatch>,
    },
    Intent {
        cursor: String,
        intent: NativeIntent,
    },
}

enum State {
    AwaitingSnapshot(VecDeque<Pending>),
    Live,
    Retired,
}

pub struct StreamSender<S: FrameSink> {
    sink: S,
    identity: FrameIdentity,
    next_seq: u32,
    acked_through: u32,
    unacked: VecDeque<Sent>,
    unacked_bytes: usize,
    snapshot_end_seq: Option<u32>,
    hydrated: bool,
    last_cursor: String,
    intents_by_seq: BTreeMap<u32, String>,
    state: State,
}

impl<S: FrameSink> StreamSender<S> {
    pub fn new(sink: S, identity: FrameIdentity) -> Self {
        Self {
            sink,
            identity,
            next_seq: 1,
            acked_through: 0,
            unacked: VecDeque::new(),
            unacked_bytes: 0,
            snapshot_end_seq: None,
            hydrated: false,
            last_cursor: "0".into(),
            intents_by_seq: BTreeMap::new(),
            state: State::AwaitingSnapshot(VecDeque::new()),
        }
    }

    pub fn is_retired(&self) -> bool {
        matches!(self.state, State::Retired)
    }

    pub fn is_hydrated(&self) -> bool {
        self.hydrated
    }

    /// Whether a sent but unacknowledged frame was large enough to travel
    /// through the framework's per-view fetch cache (alpha.4 caches JSON
    /// payloads of 8 KiB or more), so it may remain there unconsumed.
    pub fn may_hold_cached_frames(&self) -> bool {
        self.unacked
            .iter()
            .any(|sent| sent.bytes >= CHANNEL_CACHE_THRESHOLD)
    }

    pub fn retire(&mut self) {
        self.state = State::Retired;
        self.unacked.clear();
        self.unacked_bytes = 0;
    }

    fn send(&mut self, body: UiFrameBody, cursor: &str) -> Result<u32, StreamError> {
        if self.is_retired() {
            return Err(StreamError::Retired);
        }
        let seq = self.next_seq;
        let frame = UiFrame {
            header: FrameHeader {
                protocol_version: UI_PROTOCOL_VERSION,
                store_generation: self.identity.store_generation.clone(),
                core_generation: self.identity.core_generation.clone(),
                subscription_id: self.identity.subscription_id.clone(),
                view_epoch: self.identity.view_epoch.clone(),
                stream_seq: seq,
                cursor: cursor.to_owned(),
            },
            body,
        };
        let bytes = serde_json::to_vec(&frame)
            .map(|encoded| encoded.len())
            .unwrap_or(usize::MAX);
        if bytes > FRAME_MAX_BYTES {
            return Err(StreamError::FrameTooLarge(bytes));
        }
        if self.unacked.len() >= WINDOW_MAX_FRAMES || self.unacked_bytes + bytes > WINDOW_MAX_BYTES
        {
            self.retire();
            return Err(StreamError::WindowExhausted);
        }
        if let Err(error) = self.sink.deliver(frame) {
            self.retire();
            return Err(StreamError::ChannelClosed(error));
        }
        self.unacked.push_back(Sent {
            seq,
            bytes,
            cursor: cursor.to_owned(),
        });
        self.unacked_bytes += bytes;
        self.next_seq += 1;
        self.last_cursor = cursor.to_owned();
        Ok(seq)
    }

    /// Enforces the snapshot bounds before `SnapshotBegin`, emits the complete
    /// snapshot, then any changes that arrived while it was being attached.
    pub fn emit_snapshot(
        &mut self,
        cursor: &str,
        snapshot: &FleetSnapshot,
    ) -> Result<(), StreamError> {
        let json = serde_json::to_string(snapshot)
            .map_err(|error| StreamError::ChannelClosed(error.to_string()))?;
        let chunks = chunk_for_frames(&json, CHUNK_BUDGET);
        let frames = chunks.len() + 2;
        if json.len() > SNAPSHOT_MAX_BYTES || frames > SNAPSHOT_MAX_FRAMES {
            return Err(StreamError::SnapshotExceedsBound {
                bytes: json.len(),
                frames,
            });
        }
        self.send(
            UiFrameBody::SnapshotBegin {
                view_revision: snapshot.view_revision.clone(),
                chunk_count: chunks.len() as u32,
                total_bytes: json.len() as u32,
            },
            cursor,
        )?;
        for (index, data) in chunks.iter().enumerate() {
            self.send(
                UiFrameBody::SnapshotChunk {
                    index: index as u32,
                    data: (*data).to_owned(),
                },
                cursor,
            )?;
        }
        let end = self.send(
            UiFrameBody::SnapshotEnd {
                view_revision: snapshot.view_revision.clone(),
            },
            cursor,
        )?;
        self.snapshot_end_seq = Some(end);
        let buffered = match std::mem::replace(&mut self.state, State::Live) {
            State::AwaitingSnapshot(buffered) => buffered,
            other => {
                self.state = other;
                VecDeque::new()
            }
        };
        for pending in buffered {
            match pending {
                Pending::Patch { cursor, patch } => self.push_patch(&cursor, patch)?,
                Pending::Intent { cursor, intent } => self.push_intent(&cursor, intent)?,
            }
        }
        Ok(())
    }

    fn buffer(&mut self, pending: Pending) -> Result<bool, StreamError> {
        if let State::AwaitingSnapshot(queue) = &mut self.state {
            if queue.len() >= MAX_BUFFERED_BEFORE_SNAPSHOT {
                self.retire();
                return Err(StreamError::BufferOverflow);
            }
            queue.push_back(pending);
            return Ok(true);
        }
        Ok(false)
    }

    pub fn push_patch(
        &mut self,
        cursor: &str,
        patch: Box<ProjectionPatch>,
    ) -> Result<(), StreamError> {
        if self.buffer(Pending::Patch {
            cursor: cursor.to_owned(),
            patch: patch.clone(),
        })? {
            return Ok(());
        }
        self.send(UiFrameBody::ProjectionPatch { patch: *patch }, cursor)
            .map(|_| ())
    }

    pub fn push_intent(&mut self, cursor: &str, intent: NativeIntent) -> Result<(), StreamError> {
        if self.buffer(Pending::Intent {
            cursor: cursor.to_owned(),
            intent: intent.clone(),
        })? {
            return Ok(());
        }
        let intent_id = intent.intent_id.clone();
        let seq = self.send(UiFrameBody::NativeIntent { intent }, cursor)?;
        self.intents_by_seq.insert(seq, intent_id);
        Ok(())
    }

    /// Heartbeats flow only after hydration; they occupy the window like any
    /// frame, so a renderer that stops acknowledging is retired.
    pub fn heartbeat(&mut self) -> Result<(), StreamError> {
        if !self.hydrated || !matches!(self.state, State::Live) {
            return Ok(());
        }
        let cursor = self.last_cursor.clone();
        self.send(UiFrameBody::BridgeHeartbeat, &cursor).map(|_| ())
    }

    /// Validates an application ACK: strictly increasing, actually sent, and
    /// naming exactly the cursor that frame represented (SPEC §18.4).
    pub fn ack(&mut self, seq: u32, cursor: &str) -> Result<AckOutcome, AckError> {
        if self.is_retired() {
            return Err(AckError::Retired);
        }
        if seq <= self.acked_through {
            return Err(AckError::NotMonotonic {
                acked_through: self.acked_through,
            });
        }
        if seq >= self.next_seq {
            return Err(AckError::FutureSequence {
                last_sent: self.next_seq - 1,
            });
        }
        let expected = self
            .unacked
            .iter()
            .find(|sent| sent.seq == seq)
            .map(|sent| sent.cursor.clone())
            .unwrap_or_default();
        if expected != cursor {
            return Err(AckError::CursorMismatch { expected });
        }
        while self.unacked.front().is_some_and(|sent| sent.seq <= seq) {
            if let Some(sent) = self.unacked.pop_front() {
                self.unacked_bytes -= sent.bytes;
            }
        }
        self.acked_through = seq;
        let newly_hydrated = !self.hydrated && self.snapshot_end_seq.is_some_and(|end| end <= seq);
        if newly_hydrated {
            self.hydrated = true;
        }
        let consumed: Vec<u32> = self
            .intents_by_seq
            .range(..=seq)
            .map(|(seq, _)| *seq)
            .collect();
        let consumed_intents = consumed
            .iter()
            .filter_map(|seq| self.intents_by_seq.remove(seq))
            .collect();
        Ok(AckOutcome {
            acknowledged_through: seq,
            hydrated: self.hydrated,
            newly_hydrated,
            consumed_intents,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use threadspace_contracts::projection::{AttentionCounts, IntentAction, IntentSource};

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<UiFrame>>>);

    impl FrameSink for Sink {
        fn deliver(&self, frame: UiFrame) -> Result<(), String> {
            self.0
                .lock()
                .map_err(|_| "poisoned".to_string())?
                .push(frame);
            Ok(())
        }
    }

    fn sender() -> (StreamSender<Sink>, Sink) {
        let sink = Sink::default();
        let identity = FrameIdentity {
            store_generation: "store".into(),
            core_generation: "core".into(),
            subscription_id: "sub".into(),
            view_epoch: "epoch".into(),
        };
        (StreamSender::new(sink.clone(), identity), sink)
    }

    fn snapshot(padding: usize) -> FleetSnapshot {
        FleetSnapshot {
            view_revision: "7".into(),
            sessions: Vec::new(),
            attention: vec![],
            counts: AttentionCounts {
                needs_attention: padding as u32,
                awaiting_action: 0,
            },
            complete: true,
            total_sessions: 0,
            total_attention: 0,
            sessions_after: None,
            attention_after: None,
        }
    }

    fn patch(to: &str) -> Box<ProjectionPatch> {
        Box::new(ProjectionPatch {
            from_cursor: "7".into(),
            to_cursor: to.into(),
            view_revision: to.into(),
            session_upserts: vec![],
            attention_upserts: vec![],
            tombstones: vec![],
            counts: AttentionCounts {
                needs_attention: 0,
                awaiting_action: 1,
            },
            page_invalidations: Vec::new(),
        })
    }

    #[test]
    fn snapshot_then_buffered_patch_in_order() {
        let (mut stream, sink) = sender();
        stream.push_patch("8", patch("8")).expect("buffered");
        stream.emit_snapshot("7", &snapshot(1)).expect("snapshot");
        let frames = sink.0.lock().expect("lock").clone();
        let kinds: Vec<&str> = frames
            .iter()
            .map(|frame| match frame.body {
                UiFrameBody::SnapshotBegin { .. } => "begin",
                UiFrameBody::SnapshotChunk { .. } => "chunk",
                UiFrameBody::SnapshotEnd { .. } => "end",
                UiFrameBody::ProjectionPatch { .. } => "patch",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["begin", "chunk", "end", "patch"]);
        let seqs: Vec<u32> = frames.iter().map(|frame| frame.header.stream_seq).collect();
        assert_eq!(seqs, [1, 2, 3, 4], "contiguous from 1");
        assert_eq!(frames[3].header.cursor, "8");
    }

    #[test]
    fn ack_rules() {
        let (mut stream, _) = sender();
        stream.emit_snapshot("7", &snapshot(1)).expect("snapshot");
        assert!(matches!(
            stream.ack(9, "7"),
            Err(AckError::FutureSequence { .. })
        ));
        assert!(matches!(
            stream.ack(3, "8"),
            Err(AckError::CursorMismatch { .. })
        ));
        let outcome = stream.ack(3, "7").expect("ack end");
        assert!(outcome.newly_hydrated && outcome.hydrated);
        assert!(matches!(
            stream.ack(3, "7"),
            Err(AckError::NotMonotonic { .. })
        ));
    }

    #[test]
    fn hydration_requires_snapshot_end() {
        let (mut stream, _) = sender();
        stream.emit_snapshot("7", &snapshot(1)).expect("snapshot");
        let partial = stream.ack(2, "7").expect("ack chunk");
        assert!(!partial.hydrated);
        stream.heartbeat().expect("no heartbeat before hydration");
        assert!(stream.ack(3, "7").expect("ack end").newly_hydrated);
    }

    #[test]
    fn window_exhaustion_retires_instead_of_buffering() {
        let (mut stream, _) = sender();
        stream.emit_snapshot("7", &snapshot(1)).expect("snapshot");
        stream.ack(3, "7").expect("hydrate");
        for _ in 0..WINDOW_MAX_FRAMES {
            stream.heartbeat().expect("within window");
        }
        assert_eq!(stream.heartbeat(), Err(StreamError::WindowExhausted));
        assert!(stream.is_retired());
        assert_eq!(stream.ack(4, "7"), Err(AckError::Retired));
    }

    #[test]
    fn intents_are_consumed_by_ack() {
        let (mut stream, _) = sender();
        stream.emit_snapshot("7", &snapshot(1)).expect("snapshot");
        stream.ack(3, "7").expect("hydrate");
        let intent = NativeIntent {
            intent_id: "intent-1".into(),
            action: IntentAction::OpenAttention {
                attention_id: "a".into(),
                session_id: "s".into(),
                outstanding: true,
                source: IntentSource::NotificationResponse,
                route: None,
                observation_enabled: true,
            },
        };
        stream.push_intent("7", intent).expect("intent");
        let outcome = stream.ack(4, "7").expect("ack intent");
        assert_eq!(outcome.consumed_intents, ["intent-1"]);
    }

    #[test]
    fn oversized_snapshot_is_refused_before_begin() {
        let (mut stream, sink) = sender();
        let mut big = snapshot(1);
        big.view_revision = "x".repeat(SNAPSHOT_MAX_BYTES);
        assert!(matches!(
            stream.emit_snapshot("7", &big),
            Err(StreamError::SnapshotExceedsBound { .. })
        ));
        assert!(sink.0.lock().expect("lock").is_empty(), "nothing sent");
    }

    #[test]
    fn escape_heavy_snapshots_still_fit_frames() {
        use threadspace_contracts::projection::{
            ExecutionPresence, ObservationState, SessionView, TurnState,
        };

        let (mut stream, sink) = sender();
        let mut heavy = snapshot(1);
        // Every character doubles when escaped twice (snapshot JSON, then chunk string).
        heavy.sessions.push(SessionView {
            session_id: "s".into(),
            provider: "synthetic".into(),
            native_session_id: "n".into(),
            display_name: "\"\\".repeat(55_000),
            activation: None,
            turn_state: TurnState::Unknown,
            execution_presence: ExecutionPresence::Unknown,
            observation: ObservationState::Unknown,
            observer_tier: None,
            observer_version: None,
            link_conflict: false,
            process: None,
            binding: None,
            live_bindings: 0,
            last_invalidation: None,
            provider_status: None,
            provider_waiting_for: None,
            last_route: None,
            fixture: true,
            revision: "1".into(),
        });
        stream
            .emit_snapshot("7", &heavy)
            .expect("fits within bounds");
        for frame in sink.0.lock().expect("lock").iter() {
            assert!(serde_json::to_vec(frame).expect("json").len() <= FRAME_MAX_BYTES);
        }
    }

    #[test]
    fn chunking_preserves_text() {
        let text = "aé\"\\\u{1}b".repeat(1000);
        let chunks = chunk_for_frames(&text, 100);
        assert_eq!(chunks.concat(), text);
        assert!(chunks.iter().all(|chunk| {
            chunk
                .chars()
                .map(threadspace_contracts::frames::escaped_len)
                .sum::<usize>()
                <= 100
        }));
    }
}
