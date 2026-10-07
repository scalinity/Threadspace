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

/// Capture ingress bounds (SPEC §8.2–§8.5).
pub mod capture {
    /// Provider raw input parsed by the capture helper.
    pub const RAW_INPUT_MAX_BYTES: usize = 8 * 1024 * 1024;
    /// JSON nesting depth parsed from provider input.
    pub const RAW_DEPTH_MAX: usize = 32;
    /// One normalized observation.
    pub const OBSERVATION_MAX_BYTES: usize = 16 * 1024;
    /// One event-socket frame (a mod batch at most).
    pub const FRAME_MAX_BYTES: usize = 64 * 1024;
    /// Records in one mod batch.
    pub const BATCH_MAX_RECORDS: usize = 128;
    /// The typed mod-batch receipt printed for the calling mod.
    pub const RECEIPT_MAX_BYTES: usize = 16 * 1024;
    /// Healthy capture: normal p95 target is 25 ms; this is the wall budget.
    pub const WALL_BUDGET_MS: u64 = 250;
    pub const CONNECT_BUDGET_MS: u64 = 20;
    pub const RECEIPT_BUDGET_MS: u64 = 75;
    /// Local spool bounds, whichever is reached first.
    pub const SPOOL_MAX_RECORDS: usize = 100_000;
    pub const SPOOL_MAX_BYTES: u64 = 256 * 1024 * 1024;
    pub const SPOOL_MAX_AGE_MS: i64 = 7 * 24 * 60 * 60 * 1000;
    /// Optional activity detail is dropped first, above this share of a bound.
    pub const SPOOL_LOW_PRIORITY_PERCENT: u64 = 80;
    /// Saturation markers kept (each names one dropped record).
    pub const SPOOL_MAX_MARKERS: usize = 10_000;
}
