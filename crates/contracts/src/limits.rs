//! Frozen transport bounds (SPEC §18.3, §18.4, §18.5).

/// Complete initial snapshot cap, enforced before `SnapshotBegin` is sent.
pub const SNAPSHOT_MAX_BYTES: usize = 512 * 1024;
/// Maximum frames in one snapshot sequence, `SnapshotBegin` through `SnapshotEnd`.
pub const SNAPSHOT_MAX_FRAMES: usize = 10;
/// Maximum serialized size of any one renderer stream frame.
pub const FRAME_MAX_BYTES: usize = 64 * 1024;
/// Unacknowledged sender window, in frames.
pub const WINDOW_MAX_FRAMES: usize = 32;
/// Unacknowledged sender window, in serialized bytes.
pub const WINDOW_MAX_BYTES: usize = 2 * 1024 * 1024;
/// Data queries in flight per native view.
pub const QUERY_MAX_IN_FLIGHT: usize = 4;
/// Serialized cap for one query reply.
pub const QUERY_REPLY_MAX_BYTES: usize = 64 * 1024;
/// Visible bridge heartbeat interval.
pub const HEARTBEAT_INTERVAL_MS: u32 = 2_000;
/// Initial hydration deadline.
pub const HYDRATION_DEADLINE_MS: u32 = 5_000;
/// Bounded detail text carried by a typed error.
pub const ERROR_DETAIL_MAX_CHARS: usize = 240;

/// One frame on the private companion control socket. The largest message is
/// a complete bounded snapshot (≤ `SNAPSHOT_MAX_BYTES`) plus its envelope.
pub const CONTROL_FRAME_MAX_BYTES: usize = 1024 * 1024;
/// Qualification and owner-supplied labels.
pub const LABEL_MAX_CHARS: usize = 120;
