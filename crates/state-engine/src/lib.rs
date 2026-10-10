//! The canonical state engine (SPEC §2.3, §5.5).
//!
//! Pure over admitted facts, checkpoints and owner commands: it performs no
//! network, filesystem, clock, randomness or OS calls. Admission resolves
//! native keys to canonical IDs (`resolve`); the engine reduces the resolved
//! facts (`Engine::apply`); `semantic` normalizes a state by native keys for
//! permutation equality; `hash` gives the exact replay digest.

pub mod causal;
pub mod command;
pub mod engine;
pub mod hash;
pub mod ids;
pub mod keys;
pub mod normalize;
pub mod profiles;
mod reduce;
pub mod resolve;
pub mod semantic;
#[cfg(feature = "synthetic")]
pub mod synthetic;
pub mod validate;
pub mod wait;

pub use engine::{Changed, Engine, Note, ReduceOutput};
pub use reduce::record_evidence;

/// The reduction rules this build applies; checkpoints record it. A store
/// whose newest checkpoint has an earlier version is upgraded when opened
/// (`Engine::upgrade`); one with a later version is refused.
pub const REDUCER_VERSION: u32 = 4;

/// The M0 fixture worker's synthetic namespace.
pub const FIXTURE_PROVIDER: &str = "synthetic";
pub const FIXTURE_PROFILE: &str = "m0a-fixture";
/// Synthetic profiles whose sessions are fixtures, never provider-observed:
/// the M0A worker and the M0C qualification stream/population sessions.
pub const FIXTURE_PROFILES: &[&str] = &[FIXTURE_PROFILE, "m0c-synthetic"];
