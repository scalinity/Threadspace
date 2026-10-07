//! Canonical domain contracts (SPEC §3, §5, §7, §8, §9).
//!
//! These Rust types are authoritative for the journal, checkpoints, the
//! capture relay and owner commands. `cargo test -p threadspace-contracts`
//! regenerates their TypeScript definitions (ts-rs, into
//! `apps/desktop/src/contracts/generated/`) and their JSON Schemas
//! (`json_schemas()`, into `crates/contracts/schemas/`), so all three
//! descriptions come from one source.

pub mod capture;
pub mod causal;
pub mod command;
pub mod envelope;
pub mod fact;
pub mod keys;
pub mod records;

use schemars::{JsonSchema, generate::SchemaSettings};

/// Version of the canonical fact payloads journaled by this build.
pub const FACT_PAYLOAD_VERSION: u32 = 1;

/// The contract versions this build writes, recorded in M1 evidence.
pub const VERSION_CATALOG: &[(&str, u32)] = &[
    ("observationEnvelope", envelope::OBSERVATION_SCHEMA_VERSION),
    ("factPayload", FACT_PAYLOAD_VERSION),
    ("captureProtocol", capture::CAPTURE_PROTOCOL_VERSION),
    ("modBatchReceipt", capture::MOD_BATCH_RECEIPT_VERSION),
];

fn schema_for<T: JsonSchema>() -> serde_json::Value {
    let generator = SchemaSettings::draft2020_12().into_generator();
    serde_json::to_value(generator.into_root_schema_for::<T>()).unwrap_or_default()
}

/// Every root canonical contract and its generated JSON Schema, by file name.
pub fn json_schemas() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "ObservationEnvelope.schema.json",
            schema_for::<envelope::ObservationEnvelope>(),
        ),
        (
            "NativeFactDraft.schema.json",
            schema_for::<fact::NativeFactDraft>(),
        ),
        ("JournalEntry.schema.json", schema_for::<fact::JournalEntry>()),
        (
            "CanonicalState.schema.json",
            schema_for::<records::CanonicalState>(),
        ),
        (
            "OwnerCommand.schema.json",
            schema_for::<command::OwnerCommand>(),
        ),
        (
            "CaptureBatch.schema.json",
            schema_for::<capture::CaptureBatch>(),
        ),
        (
            "CaptureReply.schema.json",
            schema_for::<capture::CaptureReply>(),
        ),
        (
            "ModBatchReceipt.schema.json",
            schema_for::<capture::ModBatchReceipt>(),
        ),
    ]
}
