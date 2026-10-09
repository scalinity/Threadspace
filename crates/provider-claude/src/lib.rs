//! Claude provider adapter for the direct interactive CLI profile (SPEC §4.4,
//! §11.3): the supported native inventory, the bracketed session→process join
//! and reconciliation planning. Pure over its `Inventory` and `Sampler`
//! inputs, so synthetic races exercise the same logic as native runs. It never
//! starts, resumes or prompts a provider session. `setup` is the reversible
//! integration installer (SPEC §19.2), the one part that writes files.

pub mod discovery;
pub mod hooks;
pub mod inventory;
pub mod profiles;
pub mod reconcile;
pub mod setup;
