//! Claude provider adapter for the direct interactive CLI profile (SPEC §4.4,
//! §11.3): the supported native inventory, the bracketed session→process join
//! and reconciliation planning. Pure over its `Inventory` and `Sampler`
//! inputs, so synthetic races exercise the same logic as native runs. It never
//! starts, resumes or prompts a provider session.

pub mod discovery;
pub mod hooks;
pub mod inventory;
pub mod reconcile;
