//! Contract coherence evidence: the version catalog, migration and
//! generated-artifact digests, JSON Schema freshness, and validation of real
//! instances (every fixture envelope, normalized draft, journal entry,
//! canonical state and owner command) against the schemas generated from the
//! Rust contracts. TypeScript definitions are generated from the same types by
//! ts-rs; their digests are recorded and `cargo test -p threadspace-contracts`
//! regenerates them (a clean `git status` afterwards is the freshness check).

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value, json};
use threadspace_contracts::canonical::command::{OwnerAction, OwnerCommand};
use threadspace_contracts::canonical::{VERSION_CATALOG, json_schemas};
use threadspace_journal::{SCHEMA_VERSION, migration_catalog};
use threadspace_relay::locator::LOCATOR_SCHEMA;
use threadspace_state_engine::REDUCER_VERSION;
use threadspace_state_engine::synthetic::normalize;
use threadspace_synthetic::builder::Step;
use threadspace_synthetic::scenarios::catalog;
use threadspace_synthetic::sqlite::{SqliteRunner, TempStore};

use crate::evidence::{Area, sha256};

/// A JSON Schema validator for the subset schemars emits: `type`, `enum`,
/// `const`, `properties`/`required`/`additionalProperties`, `items`,
/// `oneOf`/`anyOf`/`allOf`, local `$ref`, and integer `minimum`/`maximum`.
struct Validator<'a> {
    root: &'a Value,
}

impl Validator<'_> {
    fn resolve<'s>(&'s self, schema: &'s Value) -> &'s Value {
        match schema.get("$ref").and_then(Value::as_str) {
            Some(reference) => reference
                .strip_prefix("#/$defs/")
                .and_then(|name| self.root.get("$defs").and_then(|d| d.get(name)))
                .map_or(schema, |target| self.resolve(target)),
            None => schema,
        }
    }

    fn type_ok(kind: &str, value: &Value) -> bool {
        match kind {
            "null" => value.is_null(),
            "boolean" => value.is_boolean(),
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            _ => true,
        }
    }

    fn check(&self, schema: &Value, value: &Value, path: &str, errors: &mut Vec<String>) {
        let schema = self.resolve(schema);
        if schema == &Value::Bool(true) {
            return;
        }
        if let Some(kind) = schema.get("type") {
            let ok = match kind {
                Value::String(k) => Self::type_ok(k, value),
                Value::Array(kinds) => kinds.iter().filter_map(Value::as_str).any(|k| Self::type_ok(k, value)),
                _ => true,
            };
            if !ok {
                errors.push(format!("{path}: expected type {kind}"));
                return;
            }
        }
        if let Some(options) = schema.get("enum").and_then(Value::as_array)
            && !options.contains(value)
        {
            errors.push(format!("{path}: {value} not in enum"));
        }
        if let Some(constant) = schema.get("const")
            && constant != value
        {
            errors.push(format!("{path}: expected const {constant}"));
        }
        if let Some(n) = value.as_i64() {
            if schema.get("minimum").and_then(Value::as_i64).is_some_and(|m| n < m) {
                errors.push(format!("{path}: below minimum"));
            }
            if schema.get("maximum").and_then(Value::as_i64).is_some_and(|m| n > m) {
                errors.push(format!("{path}: above maximum"));
            }
        }
        if let Some(object) = value.as_object() {
            let properties = schema.get("properties").and_then(Value::as_object);
            for required in schema.get("required").and_then(Value::as_array).into_iter().flatten() {
                if let Some(name) = required.as_str()
                    && !object.contains_key(name)
                {
                    errors.push(format!("{path}: missing {name}"));
                }
            }
            for (key, item) in object {
                match properties.and_then(|p| p.get(key)) {
                    Some(property) => self.check(property, item, &format!("{path}.{key}"), errors),
                    None => match schema.get("additionalProperties") {
                        Some(Value::Bool(false)) => errors.push(format!("{path}: unexpected {key}")),
                        Some(extra @ Value::Object(_)) => self.check(extra, item, &format!("{path}.{key}"), errors),
                        _ => {}
                    },
                }
            }
        }
        if let (Some(items), Some(array)) = (schema.get("items"), value.as_array()) {
            for (index, item) in array.iter().enumerate() {
                self.check(items, item, &format!("{path}[{index}]"), errors);
            }
        }
        let passes = |s: &Value| {
            let mut nested = Vec::new();
            self.check(s, value, path, &mut nested);
            nested.is_empty()
        };
        if let Some(all) = schema.get("allOf").and_then(Value::as_array) {
            for s in all {
                self.check(s, value, path, errors);
            }
        }
        if let Some(any) = schema.get("anyOf").and_then(Value::as_array)
            && !any.iter().any(passes)
        {
            errors.push(format!("{path}: matches no anyOf branch"));
        }
        if let Some(one) = schema.get("oneOf").and_then(Value::as_array) {
            let matched = one.iter().filter(|s| passes(s)).count();
            if matched != 1 {
                errors.push(format!("{path}: matches {matched} oneOf branches"));
            }
        }
    }

    fn validate(root: &Value, value: &Value) -> Vec<String> {
        let validator = Validator { root };
        let mut errors = Vec::new();
        validator.check(root, value, "$", &mut errors);
        errors
    }
}

fn digest_dir(dir: &Path, extension: &str) -> Result<(BTreeMap<String, String>, String), String> {
    let mut files = BTreeMap::new();
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|e| e.to_str()) == Some(extension) {
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            files.insert(
                path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                sha256(&bytes),
            );
        }
    }
    let aggregate = sha256(files.iter().map(|(n, h)| format!("{n} {h}\n")).collect::<String>().as_bytes());
    Ok((files, aggregate))
}

pub fn run(repo: &Path, root: &Path) -> Result<Value, String> {
    let area = Area::new(root, "contracts")?;
    let schemas: BTreeMap<&str, Value> = json_schemas().into_iter().collect();
    let schema_dir = repo.join("crates/contracts/schemas");
    let mut fresh = Map::new();
    for (name, schema) in &schemas {
        let mut rendered = serde_json::to_string_pretty(schema).map_err(|e| e.to_string())?;
        rendered.push('\n');
        let on_disk = std::fs::read_to_string(schema_dir.join(name)).unwrap_or_default();
        fresh.insert((*name).to_owned(), json!(on_disk == rendered));
    }
    let (ts_files, ts_aggregate) = digest_dir(&repo.join("apps/desktop/src/contracts/generated"), "ts")?;
    let (schema_files, schema_aggregate) = digest_dir(&schema_dir, "json")?;

    // Validate real instances against the generated schemas.
    let mut counts: BTreeMap<&str, (usize, Vec<String>)> = BTreeMap::new();
    let mut validate = |schema: &'static str, value: Value, label: &str| {
        let errors = Validator::validate(&schemas[schema], &value);
        let entry = counts.entry(schema).or_default();
        entry.0 += 1;
        entry.1.extend(errors.into_iter().take(3).map(|e| format!("{label}: {e}")));
    };
    for scenario in catalog() {
        for step in &scenario.steps {
            if let Step::Observe(envelope) = step {
                validate("ObservationEnvelope.schema.json", json!(envelope), &scenario.name);
                for draft in normalize(envelope).unwrap_or_default() {
                    validate("NativeFactDraft.schema.json", json!(draft), &scenario.name);
                }
            }
        }
        let order: Vec<usize> = (0..scenario.steps.len()).collect();
        let mut sqlite = SqliteRunner::open(TempStore::new("contracts"), 11).map_err(|e| e.to_string())?;
        threadspace_synthetic::runner::run(&scenario, &order, &mut sqlite);
        for entry in sqlite.journal.journal_entries(0).map_err(|e| e.to_string())? {
            validate("JournalEntry.schema.json", json!(entry), &scenario.name);
        }
        validate("CanonicalState.schema.json", json!(sqlite.journal.canonical_state()), &scenario.name);
    }
    for action in [
        OwnerAction::Acknowledge,
        OwnerAction::Resolve { reason: "Mark handled".into() },
        OwnerAction::Snooze { until_ms: 1 },
    ] {
        validate(
            "OwnerCommand.schema.json",
            json!(OwnerCommand {
                command_id: "c".into(),
                attention_id: "a".into(),
                expected_revision: Some("3".into()),
                action,
            }),
            "owner",
        );
    }
    let validation: Map<String, Value> = counts
        .iter()
        .map(|(schema, (count, errors))| {
            ((*schema).to_owned(), json!({ "instances": count, "errors": errors.len(), "firstErrors": errors.iter().take(5).collect::<Vec<_>>() }))
        })
        .collect();
    let valid = counts.values().all(|(_, errors)| errors.is_empty());
    let all_fresh = fresh.values().all(|v| v == &json!(true));
    let summary = json!({
        "area": "contracts",
        "pass": valid && all_fresh,
        "versions": {
            "contracts": VERSION_CATALOG.iter().map(|(n, v)| json!({ "name": n, "version": v })).collect::<Vec<_>>(),
            "journalSchema": SCHEMA_VERSION,
            "reducer": REDUCER_VERSION,
            "runtimeLocator": LOCATOR_SCHEMA,
        },
        "migrations": migration_catalog().into_iter().map(|(id, name, sum)| json!({ "id": id, "name": name, "sha256": sum })).collect::<Vec<_>>(),
        "generated": {
            "typescript": { "dir": "apps/desktop/src/contracts/generated", "files": ts_files.len(), "aggregateSha256": ts_aggregate, "perFile": ts_files },
            "jsonSchema": { "dir": "crates/contracts/schemas", "files": schema_files.len(), "aggregateSha256": schema_aggregate, "perFile": schema_files, "freshAgainstRust": fresh },
        },
        "instanceValidation": validation,
    });
    area.json("summary.json", &summary)?;
    Ok(summary)
}
