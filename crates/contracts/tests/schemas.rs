//! Regenerates the canonical JSON Schemas into `crates/contracts/schemas/`
//! (as ts-rs regenerates the TypeScript definitions on `cargo test`). With
//! `THREADSPACE_CHECK_SCHEMAS=1` it writes nothing and fails if a committed
//! schema differs from what the Rust contracts generate.

use std::path::PathBuf;

use threadspace_contracts::canonical::json_schemas;

fn schema_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas")
}

fn render(schema: &serde_json::Value) -> String {
    let mut text = serde_json::to_string_pretty(schema).expect("schema serializes");
    text.push('\n');
    text
}

#[test]
fn json_schemas_match_the_rust_contracts() {
    let dir = schema_dir();
    let check = std::env::var("THREADSPACE_CHECK_SCHEMAS").as_deref() == Ok("1");
    if !check {
        std::fs::create_dir_all(&dir).expect("schema dir");
    }
    let mut stale = Vec::new();
    for (name, schema) in json_schemas() {
        let text = render(&schema);
        let path = dir.join(name);
        if check {
            if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
                stale.push(name);
            }
        } else {
            std::fs::write(&path, text).expect("write schema");
        }
    }
    assert!(stale.is_empty(), "stale JSON Schemas: {stale:?}");
}

#[test]
fn every_schema_is_a_draft_2020_12_object_schema() {
    for (name, schema) in json_schemas() {
        assert_eq!(
            schema["$schema"], "https://json-schema.org/draft/2020-12/schema",
            "{name}"
        );
        assert!(schema.get("title").is_some(), "{name} has a title");
    }
}

#[test]
fn unknown_fact_discriminators_do_not_deserialize() {
    use threadspace_contracts::canonical::fact::FactPayload;
    let known = serde_json::json!({ "kind": "TURN_STARTED" });
    assert!(serde_json::from_value::<FactPayload>(known).is_ok());
    let unknown = serde_json::json!({ "kind": "TURN_TELEPORTED" });
    assert!(serde_json::from_value::<FactPayload>(unknown).is_err());
    let bad_value = serde_json::json!({
        "kind": "TURN_OUTCOME_OBSERVED", "outcome": "EXPLODED", "reason": null, "summary": null
    });
    assert!(serde_json::from_value::<FactPayload>(bad_value).is_err());
}
