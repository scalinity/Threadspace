//! The fixture catalog and exact replay digests (MILESTONES M1 "Quantified
//! acceptance"). Each scenario is admitted through the real SQLite journal
//! with a fixed seed, then replayed from its checkpoint and from genesis,
//! repeatedly and across restarts; every digest must be identical.

use std::path::Path;

use serde_json::{Value, json};
use threadspace_contracts::canonical::fact::JournalEntry;
use threadspace_state_engine::engine::Engine;
use threadspace_state_engine::hash::{JournalDigest, state_hash};
use threadspace_state_engine::semantic::semantic_hash;
use threadspace_synthetic::builder::Step;
use threadspace_synthetic::runner::{PureRunner, run};
use threadspace_synthetic::scenarios::catalog;
use threadspace_synthetic::sqlite::{SqliteRunner, TempStore};

use crate::evidence::{Area, sha256};

const SEED: u64 = 0x4D31;
const REPEATS: usize = 3;

/// Writes `fixtures/m1/<scenario>.jsonl` (one step per line) and the catalog.
pub fn catalog_fixtures(repo: &Path, root: &Path) -> Result<Value, String> {
    let area = Area::new(root, "fixtures")?;
    let dir = repo.join("fixtures/m1");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    for scenario in catalog() {
        let mut text = String::new();
        for (index, step) in scenario.steps.iter().enumerate() {
            let line = match step {
                Step::Observe(envelope) => json!({ "step": index, "observe": envelope }),
                Step::Owner(owner) => json!({
                    "step": index,
                    "owner": {
                        "commandId": owner.command_id,
                        "target": format!("{:?}", owner.target),
                        "action": owner.action,
                        "atMs": owner.at_ms,
                    },
                }),
            };
            text.push_str(&serde_json::to_string(&line).map_err(|e| e.to_string())?);
            text.push('\n');
        }
        let path = dir.join(format!("{}.jsonl", scenario.name));
        std::fs::write(&path, &text).map_err(|e| e.to_string())?;
        entries.push(json!({
            "scenario": scenario.name,
            "family": scenario.family,
            "steps": scenario.steps.len(),
            "observations": scenario.steps.iter().filter(|s| matches!(s, Step::Observe(_))).count(),
            "ownerCommands": scenario.steps.iter().filter(|s| matches!(s, Step::Owner(_))).count(),
            "deliveryConstraints": scenario.constraints.len(),
            "file": format!("fixtures/m1/{}.jsonl", scenario.name),
            "sha256": sha256(text.as_bytes()),
        }));
    }
    let summary = json!({ "area": "fixtures", "scenarios": entries.len(), "catalog": entries });
    area.json("catalog.json", &summary)?;
    Ok(summary)
}

fn replay_entries(entries: &[JournalEntry]) -> Engine {
    let mut engine = Engine::empty();
    for entry in entries {
        engine.apply(entry);
    }
    engine
}

pub fn replay_hashes(root: &Path) -> Result<Value, String> {
    let area = Area::new(root, "replay")?;
    let mut rows = Vec::new();
    let mut pass = true;
    for scenario in catalog() {
        let order: Vec<usize> = (0..scenario.steps.len()).collect();
        let pure_semantic = run(&scenario, &order, &mut PureRunner::new(1)).semantic_hash;
        let store = TempStore::new("replay");
        let mut sqlite = SqliteRunner::open(store, SEED).map_err(|e| e.to_string())?;
        run(&scenario, &order, &mut sqlite);
        let live_state = state_hash(sqlite.journal.canonical_state());
        let first = sqlite.journal.replay_digest().map_err(|e| e.to_string())?;
        let entries = sqlite.journal.journal_entries(0).map_err(|e| e.to_string())?;
        let mut stable = true;
        for _ in 0..REPEATS {
            let again = sqlite.journal.replay_digest().map_err(|e| e.to_string())?;
            stable &= again == first;
            stable &= state_hash(&replay_entries(&entries).state) == live_state;
        }
        let checkpoint = sqlite.journal.checkpoint("EVIDENCE", 1).map_err(|e| e.to_string())?;
        let restarted = sqlite.restart(SEED + 1).map_err(|e| e.to_string())?;
        let after_restart = restarted.journal.replay_digest().map_err(|e| e.to_string())?;
        let restart_state = state_hash(restarted.journal.canonical_state());
        let genesis = restarted.journal.replay_from_genesis().map_err(|e| e.to_string())?;
        let mut digest = JournalDigest::default();
        let mut export = String::new();
        for entry in &entries {
            digest.add(entry);
            export.push_str(&serde_json::to_string(entry).map_err(|e| e.to_string())?);
            export.push('\n');
        }
        area.text(&format!("{}.journal.jsonl", scenario.name), &export)?;
        let ok = stable
            && first.state_sha256 == live_state
            && first.projection_sha256 == first.tables_sha256
            && after_restart.journal_sha256 == first.journal_sha256
            && restart_state == live_state
            && state_hash(&genesis) == live_state
            && first.semantic_sha256 == pure_semantic
            && digest.finish() == first.journal_sha256;
        pass &= ok;
        rows.push(json!({
            "scenario": scenario.name,
            "pass": ok,
            "reducerVersion": first.reducer_version,
            "schemaVersion": first.schema_version,
            "journal": {
                "entries": first.entries, "facts": first.facts,
                "firstCursor": first.first_cursor, "lastCursor": first.last_cursor,
                "sha256": first.journal_sha256,
                "export": format!("evidence/M1/replay/{}.journal.jsonl", scenario.name),
            },
            "checkpointSha256": checkpoint,
            "stateSha256": first.state_sha256,
            "projectionSha256": first.projection_sha256,
            "tablesSha256": first.tables_sha256,
            "semanticSha256": first.semantic_sha256,
            "pureSemanticSha256": pure_semantic,
            "repeatedReplaysStable": stable,
            "restartStateSha256": restart_state,
            "genesisReplayStateSha256": state_hash(&genesis),
            "semanticOfGenesis": semantic_hash(&genesis),
        }));
    }
    let summary = json!({
        "area": "replay",
        "pass": pass,
        "seed": SEED,
        "repeats": REPEATS,
        "contract": "Exact: the same admitted journal (or checkpoint plus later entries) replays to identical state, projection and table digests, repeatedly, across restart and from genesis. Semantic: the SQLite path's native-key projection equals the pure path's.",
        "scenarios": rows,
    });
    area.json("summary.json", &summary)?;
    Ok(summary)
}

/// Replays a journal export (one `JournalEntry` per line) from genesis.
pub fn verify(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    let mut digest = JournalDigest::default();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let entry: JournalEntry = serde_json::from_str(line).map_err(|e| e.to_string())?;
        digest.add(&entry);
        entries.push(entry);
    }
    let engine = replay_entries(&entries);
    Ok(json!({
        "entries": entries.len(),
        "journalSha256": digest.finish(),
        "stateSha256": state_hash(&engine.state),
        "semanticSha256": semantic_hash(&engine.state),
    }))
}
