//! Threadspace IPC and view-model contracts.
//!
//! These Rust types are authoritative for the renderer bridge (SPEC §18.3–18.4)
//! and for the private companion control protocol (SPEC §8.1). Running
//! `cargo test -p threadspace-contracts` regenerates the TypeScript definitions
//! in `apps/desktop/src/contracts/generated/`; the frontend still validates
//! every frame at runtime because a TypeScript type is not wire validation.

pub mod control;
pub mod cursor;
pub mod diagnostics;
pub mod limits;
pub mod projection;
pub mod route;
pub mod ui;
