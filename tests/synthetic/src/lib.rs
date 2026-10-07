//! Deterministic synthetic provider and replay harness (SPEC §21.3).
//!
//! Native-shaped histories (`builder`, `scenarios`) are delivered through an
//! admission boundary (`runner`) in seeded valid orders (`permute`), with a
//! virtual clock and seeded allocation (`rng`), and no model calls.

pub mod builder;
pub mod permute;
pub mod rng;
pub mod runner;
pub mod scenarios;
pub mod view;

/// Sensitive bodies planted in fixtures; no persisted record may contain them.
pub const SECRET_PROMPT: &str = "SECRET-PROMPT-BODY-7f3a do not persist";
pub const SECRET_TOOL: &str = "SECRET-TOOL-PAYLOAD-91c2 rm -rf ~/private";
