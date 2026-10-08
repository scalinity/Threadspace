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

/// The seeds that failed the first permutation run (fixtures/m1/
/// regression-seeds.json), replayed through SQLite with the materialized
/// tables checked against the state after every step.
#[test]
fn preserved_failing_seeds_now_keep_tables_equal_to_state_at_every_step() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/m1/regression-seeds.json"))
        .expect("regression seeds");
    let fixture: serde_json::Value = serde_json::from_str(&text).expect("json");
    let scenarios = catalog();
    let seeds = fixture["seeds"].as_array().expect("seeds");
    assert_eq!(seeds.len(), 17);
    for entry in seeds {
        let name = entry["scenario"].as_str().expect("scenario");
        let seed = entry["seed"].as_u64().expect("seed");
        let scenario = scenarios.iter().find(|s| s.name == name).expect("known scenario");
        let (order, _) = threadspace_synthetic::permute::order_for(scenario, seed);
        let mut sqlite = SqliteRunner::open(TempStore::new("regression"), seed).expect("open");
        let report = threadspace_synthetic::runner::run_observed(scenario, &order, &mut sqlite, |runner| {
            runner
                .journal
                .projection_differences()
                .expect("diff")
                .into_iter()
                .map(|(table, _, _)| format!("{table} differs"))
                .collect()
        });
        assert!(report.violations.is_empty(), "{name} seed {seed}: {:?}", report.violations);
    }
}

/// Ingest cursors are local commit positions: a jump in them is not
/// provider event loss, a completion or an invalidation, and raises nothing.
/// A source-sequence gap is a coverage gap of that source epoch only.
#[test]
fn ingest_cursor_gaps_raise_nothing_and_source_gaps_stay_scoped() {
    use threadspace_synthetic::builder::{Builder, session};
    let store = TempStore::new("cursor-gap");
    let path = store.journal_path();
    let mut sqlite = SqliteRunner::open(store, 21).expect("open");
    let mut b = Builder::new("cursor-gap", "snapshot-live");
    let s = session("claude-like", "sess-cursor");
    b.obs(Some(&s), "session.start").push();
    b.obs(Some(&s), "turn.start").turn("t1").push();
    // Inventory source: sequences 1, 2 then 4 (3 missing).
    for (sequence, present) in [(1, true), (2, true), (4, true)] {
        b.obs(Some(&s), "inventory.row")
            .source("synthetic.inventory", "inv-epoch-1")
            .sequence(sequence)
            .payload(serde_json::json!({ "present": present, "row": null, "interval": { "startMs": 1, "endMs": 2 } }))
            .push();
    }
    b.obs(Some(&s), "turn.step").turn("t1").push();
    let scenario = b.build(|_| Ok(()));
    let steps: Vec<usize> = (0..scenario.steps.len()).collect();
    let (first, rest) = steps.split_at(3);
    run(&scenario, first, &mut sqlite);
    let before = sqlite.state().clone();
    // Open a 1,000-position hole in the local ingest cursor while closed.
    let restarted = {
        let SqliteRunner { journal, store, .. } = sqlite;
        drop(journal);
        let conn = rusqlite::Connection::open(&path).expect("raw");
        conn.execute("UPDATE sqlite_sequence SET seq = seq + 1000 WHERE name = 'observations'", []).expect("hole");
        drop(conn);
        SqliteRunner::open(store, 22).expect("reopen")
    };
    let mut sqlite = restarted;
    assert_eq!(state_hash(sqlite.state()), state_hash(&before), "a cursor hole changes nothing at restart");
    run(&scenario, rest, &mut sqlite);
    let state = sqlite.state();
    let cursors: Vec<i64> = sqlite.journal.journal_entries(0).expect("entries").iter().map(|e| e.cursor).collect();
    assert!(cursors.windows(2).any(|w| w[1] - w[0] > 1000), "the hole is real: {cursors:?}");
    // No diagnostic, gap fact, completion or invalidation from the hole.
    assert!(sqlite.journal.admission_diagnostics(50).expect("diag").is_empty());
    let turn = state.turns.values().next().expect("turn");
    assert_eq!(serde_json::to_value(&turn.state).expect("json"), "WORKING");
    // The inventory source's own gap is recorded for that source only.
    let gaps: Vec<_> = state.coverage.values().filter(|c| !c.gaps.is_empty()).collect();
    assert_eq!(gaps.len(), 1, "one source epoch has a gap");
    assert_eq!(gaps[0].source_id, "synthetic.inventory");
    assert_eq!((gaps[0].gaps[0].first.as_str(), gaps[0].gaps[0].last.as_str()), ("3", "3"));
    let digest = sqlite.journal.replay_digest().expect("digest");
    assert_eq!(digest.projection_sha256, digest.tables_sha256);
    assert_eq!(state_hash(&sqlite.journal.replay_from_genesis().expect("genesis")), state_hash(state));
}

#[test]
fn a_live_batch_returns_only_the_intents_still_pending_after_it() {
    use threadspace_contracts::canonical::records::OutboxState;
    use threadspace_journal::EnvelopeAdmission;
    use threadspace_state_engine::synthetic::normalize_envelope;
    use threadspace_synthetic::scenarios::waiting;
    let mut sqlite = SqliteRunner::open(TempStore::new("live-batch"), 5).expect("open");
    // Raised and ended inside one batch: the input wait and request req-1.
    let scenario = waiting();
    let admissions: Vec<EnvelopeAdmission<'_>> = scenario
        .steps
        .iter()
        .filter_map(|step| match step {
            Step::Observe(envelope) => Some(EnvelopeAdmission { envelope, normalized: normalize_envelope(envelope) }),
            Step::Owner(_) => None,
        })
        .collect();
    let outcome = sqlite.journal.admit_batch(&admissions, sqlite.delivery, 2).expect("admit");
    let state = sqlite.journal.canonical_state();
    assert_eq!(outcome.notifications.len(), 1, "only req-2's item is still eligible");
    assert!(outcome.notifications.iter().all(|n| state.outbox[&n.request_id].state == OutboxState::Pending));
}

#[test]
fn a_command_that_changes_nothing_reports_the_items_unchanged_revision() {
    use threadspace_contracts::cursor::parse_cursor;
    use threadspace_synthetic::scenarios::waiting;
    let mut sqlite = SqliteRunner::open(TempStore::new("noop-command"), 6).expect("open");
    for step in &waiting().steps {
        if let Step::Observe(envelope) = step {
            sqlite.observe(envelope);
        }
    }
    let item = sqlite
        .journal
        .canonical_state()
        .attention
        .values()
        .find(|a| !a.resolved())
        .map(|a| a.id.clone())
        .expect("an open item");
    let first = sqlite.journal.resolve_attention("cmd-1", &item, None, "handled", 10).expect("resolve");
    let again = sqlite.journal.resolve_attention("cmd-2", &item, None, "handled", 11).expect("same resolution");
    assert_eq!(again.receipt.target_revision, first.receipt.target_revision, "nothing changed");
    let revision = parse_cursor(&again.receipt.target_revision).expect("revision");
    sqlite
        .journal
        .acknowledge_attention("cmd-3", &item, Some(revision), 12)
        .expect("the reported revision is the item's current one");
}

#[test]
fn an_observation_id_not_in_canonical_form_is_refused() {
    let mut sqlite = SqliteRunner::open(TempStore::new("uppercase-id"), 7).expect("open");
    let Some(Step::Observe(envelope)) = duplicate_deliveries().steps.into_iter().next() else {
        panic!("an observation first");
    };
    let mut envelope = *envelope;
    envelope.observation_id = envelope.observation_id.to_ascii_uppercase();
    assert_ne!(envelope.observation_id, envelope.observation_id.to_ascii_lowercase(), "has hex letters");
    let receipt = sqlite.observe(&envelope);
    assert_eq!(receipt.status, RecordStatus::NotAccepted);
    assert_eq!(receipt.reason.as_deref(), Some("INVALID_OBSERVATION_ID"));
}
