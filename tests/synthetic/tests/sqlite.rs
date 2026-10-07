//! The catalog through the real single-writer SQLite journal: the same
//! semantics as the pure path, materialized tables equal to the reducer's
//! state, exact replay from genesis and across restart.

use threadspace_contracts::canonical::capture::RecordStatus;
use threadspace_contracts::canonical::command::{OwnerAction, OwnerCommand};
use threadspace_state_engine::hash::state_hash;
use threadspace_state_engine::semantic::semantic_hash;
use threadspace_synthetic::builder::Step;
use threadspace_synthetic::permute::linear_extension;
use threadspace_synthetic::runner::{Admit, PureRunner, run, target_attention};
use threadspace_synthetic::scenarios::{catalog, duplicate_deliveries, owner_commands, sensitive_payload};
use threadspace_synthetic::sqlite::{SqliteRunner, TempStore};
use threadspace_synthetic::{SECRET_PROMPT, SECRET_TOOL};

#[test]
fn every_scenario_through_sqlite_matches_the_pure_path_and_replays_exactly() {
    let mut failures = Vec::new();
    for scenario in catalog() {
        let order: Vec<usize> = (0..scenario.steps.len()).collect();
        let mut pure = PureRunner::new(1);
        let expected = run(&scenario, &order, &mut pure).semantic_hash;
        let mut sqlite = SqliteRunner::open(TempStore::new(&scenario.name), 7).expect("open");
        let report = run(&scenario, &order, &mut sqlite);
        let name = &scenario.name;
        if let Err(error) = (scenario.expect)(sqlite.state()) {
            failures.push(format!("{name}: {error}"));
        }
        failures.extend(report.violations.iter().map(|v| format!("{name}: {v}")));
        if report.semantic_hash != expected {
            failures.push(format!("{name}: SQLite semantics differ from the pure path"));
        }
        let digest = sqlite.journal.replay_digest().expect("digest");
        if digest.projection_sha256 != digest.tables_sha256 {
            failures.push(format!("{name}: materialized tables differ from the state"));
        }
        let current = state_hash(sqlite.state());
        if digest.state_sha256 != current {
            failures.push(format!("{name}: checkpoint+replay differs from live state"));
        }
        let genesis = sqlite.journal.replay_from_genesis().expect("genesis");
        if state_hash(&genesis) != current {
            failures.push(format!("{name}: replay from genesis differs"));
        }
        let restarted = sqlite.restart(8).expect("restart");
        if state_hash(restarted.state()) != current {
            failures.push(format!("{name}: state differs after restart"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn sqlite_permutations_converge_with_the_pure_reference() {
    let mut failures = Vec::new();
    for scenario in catalog() {
        let reference: Vec<usize> = (0..scenario.steps.len()).collect();
        let expected = run(&scenario, &reference, &mut PureRunner::new(1)).semantic_hash;
        for seed in 0..6u64 {
            let order = linear_extension(scenario.steps.len(), &scenario.constraints, 500 + seed);
            let mut sqlite =
                SqliteRunner::open(TempStore::new(&format!("{}-{seed}", scenario.name)), seed).expect("open");
            let report = run(&scenario, &order, &mut sqlite);
            if report.semantic_hash != expected {
                failures.push(format!("{} seed {seed}: differs", scenario.name));
            }
            failures.extend(report.violations.iter().map(|v| format!("{} seed {seed}: {v}", scenario.name)));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn ten_deliveries_of_each_observation_create_no_duplicate_sessions_or_facts() {
    let scenario = duplicate_deliveries();
    let order: Vec<usize> = (0..scenario.steps.len()).collect();
    let mut sqlite = SqliteRunner::open(TempStore::new("dup"), 3).expect("open");
    let report = run(&scenario, &order, &mut sqlite);
    let committed = report.receipts.iter().filter(|r| r.status == RecordStatus::Committed).count();
    let already = report
        .receipts
        .iter()
        .filter(|r| r.status == RecordStatus::AlreadyCommitted)
        .count();
    assert_eq!((committed, already), (3, 27), "3 observations, 27 duplicate deliveries");
    let digest = sqlite.journal.replay_digest().expect("digest");
    assert_eq!(digest.entries, 3, "one journal entry per observation");
    // session.start, turn.start and turn.complete each normalize to one fact.
    assert_eq!(digest.facts, 3, "zero duplicate facts");
    assert_eq!(sqlite.state().sessions.len(), 1, "zero duplicate sessions");
}

#[test]
fn a_committed_owner_command_retried_after_its_target_advanced_returns_the_original_result() {
    let scenario = owner_commands();
    let mut sqlite = SqliteRunner::open(TempStore::new("owner-retry"), 4).expect("open");
    // Deliver everything except the owner steps.
    for step in &scenario.steps {
        if let Step::Observe(envelope) = step {
            sqlite.observe(envelope);
        }
    }
    let target = match &scenario.steps[2] {
        Step::Observe(envelope) => threadspace_synthetic::builder::Target::TurnOutput {
            session: envelope.session_key.clone().expect("session"),
            agent: None,
            turn: "t1".into(),
        },
        Step::Owner(_) => unreachable!(),
    };
    let attention_id = target_attention(sqlite.state(), &target).expect("item");
    let revision = sqlite.state().attention[&attention_id].revision.to_string();
    let command = OwnerCommand {
        command_id: "cmd-retry".into(),
        attention_id: attention_id.clone(),
        expected_revision: Some(revision.clone()),
        action: OwnerAction::Acknowledge,
    };
    let first = sqlite.journal.admit_owner_command(&command, 1).expect("first");
    // Another command advances the same item's revision...
    sqlite
        .journal
        .admit_owner_command(
            &OwnerCommand {
                command_id: "cmd-snooze".into(),
                attention_id: attention_id.clone(),
                expected_revision: None,
                action: OwnerAction::Snooze { until_ms: 99 },
            },
            2,
        )
        .expect("snooze");
    assert_ne!(sqlite.state().attention[&attention_id].revision.to_string(), revision);
    // ...yet the retry, with its now-stale expected revision, still resolves
    // to the original committed result rather than a conflict.
    let retry = sqlite.journal.admit_owner_command(&command, 3).expect("retry");
    assert_eq!(retry.receipt.cursor, first.receipt.cursor);
    assert_eq!(
        serde_json::to_value(&retry.receipt.status).expect("json"),
        "ALREADY_COMMITTED"
    );
    assert!(retry.change.is_none(), "a retry writes nothing");
    // ...also across a restart.
    let mut restarted = sqlite.restart(5).expect("restart");
    let after = restarted.journal.admit_owner_command(&command, 4).expect("after restart");
    assert_eq!(after.receipt.cursor, first.receipt.cursor);
    // A reused ID with a different payload is a conflict.
    let conflict = restarted.journal.admit_owner_command(
        &OwnerCommand {
            action: OwnerAction::Resolve { reason: "other".into() },
            ..command
        },
        5,
    );
    assert!(conflict.is_err());
}

#[test]
fn sensitive_bodies_never_reach_the_store_files() {
    let scenario = sensitive_payload();
    let order: Vec<usize> = (0..scenario.steps.len()).collect();
    let store = TempStore::new("sensitive");
    let dir = store.dir.clone();
    let mut sqlite = SqliteRunner::open(store, 6).expect("open");
    run(&scenario, &order, &mut sqlite);
    sqlite.journal.checkpoint("TEST", 1).expect("checkpoint");
    for entry in std::fs::read_dir(&dir).expect("dir") {
        let path = entry.expect("entry").path();
        let bytes = std::fs::read(&path).expect("read");
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains(SECRET_PROMPT), "{} holds the prompt body", path.display());
        assert!(!text.contains(SECRET_TOOL), "{} holds the tool payload", path.display());
    }
    assert!(!semantic_hash(sqlite.state()).is_empty());
}
