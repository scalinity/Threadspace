//! A reducer-1 store upgrades to the same state wherever its newest
//! reducer-1 checkpoint sits. Every variant is a disposable copy of a
//! genuine reducer-1 store (`fixtures/m1/reducer-1-store`, written by
//! candidate f7e9a6c) holding the same committed history, differing only in
//! its checkpoints:
//!
//! - A: `journal.sqlite3` as committed: newest checkpoint at cursor 30, no
//!   journal suffix;
//! - B: the same without its cursor-30 checkpoint: the upgrade replays all
//!   thirty entries after the cursor-0 checkpoint;
//! - C: `journal-checkpoint-6.sqlite3` without its cursor-30 checkpoint:
//!   the upgrade replays twenty-four entries after a cursor-6 checkpoint.
//!
//! Each opens through the production path (`Journal::open_with`). The
//! upgrade keeps the notification dispositions reducer 1 committed and holds
//! eligibility it derives first: 1 PENDING, 4 HELD, 1 SUPPRESSED, asserted
//! request by request, and the three upgraded states are identical.
//!
//! `journal-resolved-at.sqlite3` holds six entries whose item reducer 1
//! kept resolved at the owner's time, while this reducer reopens it and
//! resolves it again later; it upgrades to the committed time with its
//! newest checkpoint at cursor 6, 0 or 4. Writes each comparison into the
//! directory in `THREADSPACE_WRITE_CHECKPOINT_TAIL` when set.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use threadspace_contracts::canonical::fact::Delivery;
use threadspace_contracts::canonical::records::{CanonicalState, OutboxState};
use threadspace_journal::{EnvelopeAdmission, Journal, JournalError};
use threadspace_state_engine::REDUCER_VERSION;
use threadspace_state_engine::hash::{sha256_hex, state_hash};
use threadspace_state_engine::ids::SeededAllocator;
use threadspace_state_engine::synthetic::normalize_envelope;
use threadspace_synthetic::builder::{Builder, Step, session};
use threadspace_synthetic::rng::VirtualClock;
use threadspace_synthetic::runner::Admit;
use threadspace_synthetic::scenarios::CLAUDE_LIKE;
use threadspace_synthetic::sqlite::{SqliteRunner, TempStore};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/m1/reducer-1-store").join(name)
}

/// A disposable copy of a committed store; `immutable=1` keeps its files
/// untouched.
fn copy(source: &Path, label: &str) -> TempStore {
    let store = TempStore::new(label);
    rusqlite::Connection::open_with_flags(
        format!("file:{}?immutable=1", source.display()),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .and_then(|s| s.execute("VACUUM INTO ?1", [store.journal_path().to_str().expect("utf-8")]))
    .expect("copy");
    store
}

/// Removes the reducer-1 checkpoint through `cursor`, leaving every other
/// row of the store as committed.
fn without_checkpoint_at(store: &TempStore, cursor: i64) {
    let removed = rusqlite::Connection::open(store.journal_path())
        .and_then(|c| {
            c.execute("DELETE FROM projection_checkpoints WHERE reducer_version = 1 AND through_cursor = ?1", [cursor])
        })
        .expect("remove checkpoint");
    assert_eq!(removed, 1, "exactly the cursor-{cursor} checkpoint is removed");
}

/// SHA-256 of every row of every table but the checkpoints (and their
/// row counter): the committed history a store holds.
fn history_sha256(path: &Path) -> String {
    let conn = rusqlite::Connection::open(path).expect("open");
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name <> 'projection_checkpoints' ORDER BY name")
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .expect("tables");
    let mut dump = String::new();
    for table in tables {
        let filter = if table == "sqlite_sequence" { " WHERE name <> 'projection_checkpoints'" } else { "" };
        let mut statement = conn.prepare(&format!("SELECT * FROM \"{table}\"{filter}")).expect("select");
        let columns = statement.column_count();
        let mut rows: Vec<String> = statement
            .query_map([], |row| {
                (0..columns)
                    .map(|i| row.get_ref(i).map(|v| format!("{v:?}")))
                    .collect::<Result<Vec<_>, _>>()
                    .map(|v| v.join("\u{1f}"))
            })
            .and_then(|rows| rows.collect())
            .expect("rows");
        rows.sort();
        dump.push_str(&format!("{table}\u{1e}{}\u{1d}", rows.join("\u{1e}")));
    }
    sha256_hex(dump.as_bytes())
}

fn checkpoints(path: &Path) -> Vec<Value> {
    rusqlite::Connection::open(path)
        .and_then(|c| {
            c.prepare("SELECT id, reducer_version, origin, through_cursor FROM projection_checkpoints ORDER BY id")?
                .query_map([], |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "reducerVersion": r.get::<_, u32>(1)?,
                        "origin": r.get::<_, String>(2)?,
                        "throughCursor": r.get::<_, i64>(3)?,
                    }))
                })?
                .collect()
        })
        .expect("checkpoints")
}

fn count(path: &Path, sql: &str) -> i64 {
    rusqlite::Connection::open(path).and_then(|c| c.query_row(sql, [], |r| r.get(0))).expect("count")
}

type Outbox = BTreeMap<String, (String, String, Option<String>)>;

/// The committed outbox rows: request ID to (attention ID, state, detail).
fn outbox_table(path: &Path) -> Outbox {
    rusqlite::Connection::open(path)
        .and_then(|c| {
            c.prepare("SELECT request_id, attention_id, state, outcome_detail FROM notification_outbox")?
                .query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))?
                .collect()
        })
        .expect("outbox")
}

fn counts(rows: &Outbox) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for (_, state, _) in rows.values() {
        *counts.entry(state.clone()).or_default() += 1;
    }
    counts
}

/// An attention item's wait scope: native session, native turn, and the
/// episode carrying it (`superseded` when none does).
fn scope_of(state: &CanonicalState, attention_id: &str) -> String {
    let item = &state.attention[attention_id];
    let session = &state.sessions[&item.session_id].native_session_id;
    let turn = item.turn_id.as_ref().and_then(|t| state.turns[t].native_turn_id.clone()).unwrap_or_default();
    let episode = state
        .waits
        .values()
        .flat_map(|w| &w.episodes)
        .find(|e| e.attention_id.as_deref() == Some(attention_id))
        .map_or_else(|| "superseded".into(), |e| format!("episode {}", e.index));
    format!("{session} {turn} {episode}")
}

/// Every outbox record the upgrade must leave, by stable request ID: the
/// wait scope it notifies for, reducer 1's committed state, and the state
/// after the upgrade.
const EXPECTED: [(&str, &str, Option<&str>, &str); 6] = [
    // The snoozed item reducer 1 left notifiable: its live intent stays.
    ("62917481-141c-8b01-9f08-8b9039b114d2", "sess-partial-actions t2 episode 0", Some("PENDING"), "PENDING"),
    // Reducer 1 took these as handled though Q1, Q11 or U2 was never
    // covered: each suppressed intent is eligible again, as catch-up work.
    ("4d9804b6-303f-81f5-b80d-c997326b6dc8", "sess-partial t1 episode 0", Some("SUPPRESSED"), "HELD"),
    ("60b47679-b35e-8713-8a93-887e332d3f23", "sess-partial-actions t1 episode 0", Some("SUPPRESSED"), "HELD"),
    ("c225bdd2-c3d5-8948-8fb9-777919d8656d", "sess-unordered-wait t1 episode 0", Some("SUPPRESSED"), "HELD"),
    // C5 moved P10 into episode 1; the superseded episode's intent stays
    // suppressed.
    ("d2f458a7-c106-8216-938e-665fa5b6d380", "sess-merge t1 superseded", Some("SUPPRESSED"), "SUPPRESSED"),
    // Episode 1 holds P10 and the uncovered Q2: reducer 1 never created its
    // intent, so the upgrade creates it held.
    ("e976fef2-0ab9-86de-86e5-5eb61c2cadc1", "sess-merge t1 episode 1", None, "HELD"),
];

/// JSON paths at which two values differ.
fn differences(a: &Value, b: &Value, path: &str, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for key in x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))) {
                let (x, y) = (x.get(key).unwrap_or(&Value::Null), y.get(key).unwrap_or(&Value::Null));
                differences(x, y, &format!("{path}.{key}"), out);
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (x, y)) in x.iter().zip(y).enumerate() {
                differences(x, y, &format!("{path}[{i}]"), out);
            }
        }
        _ if a != b => out.push(format!("{path}: {a} | {b}")),
        _ => {}
    }
}

/// A new live wait in a new session, admitted after the upgrade.
fn live_wait() -> Vec<Step> {
    let mut b = Builder::new("after-upgrade", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-after-upgrade");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(1).push();
    b.obs(Some(&s), "wait").turn("t1").sequence(2).payload(json!({ "category": "INPUT", "signal": "POSITIVE" })).push();
    b.build(|_| Ok(())).steps
}

struct Opened {
    variant: &'static str,
    report: Value,
    state: CanonicalState,
    sqlite: SqliteRunner,
}

/// Opens a variant under this reducer and records what the upgrade left.
fn open(variant: &'static str, store: TempStore, committed: &Outbox) -> Opened {
    let path = store.journal_path();
    let before = checkpoints(&path);
    let cursor = before.last().and_then(|c| c["throughCursor"].as_i64()).expect("checkpoint");
    let suffix = count(&path, &format!("SELECT COUNT(*) FROM observations WHERE canonical = 1 AND ingest_seq > {cursor}"));
    let attention_before = count(&path, "SELECT COUNT(*) FROM attention_items");
    let commands_before = count(&path, "SELECT COUNT(*) FROM attention_commands");

    let sqlite = SqliteRunner::open(store, 32).expect("open and upgrade");
    let state = sqlite.state().clone();
    let upgraded = outbox_table(&path);
    let records: Vec<Value> = upgraded
        .iter()
        .map(|(request, (attention, now, detail))| {
            json!({
                "requestId": request,
                "attentionId": attention,
                "scope": scope_of(&state, attention),
                "reducer1": committed.get(request).map(|(_, s, _)| s),
                "upgraded": now,
                "detail": detail,
            })
        })
        .collect();
    let report = json!({
        "variant": variant,
        "checkpointsBefore": before,
        "suffixEntriesReplayed": suffix,
        "historySha256": history_sha256(&path),
        "outbox": records,
        "counts": counts(&upgraded),
        "attentionItems": [attention_before, state.attention.len()],
        "ownerCommands": [commands_before, count(&path, "SELECT COUNT(*) FROM attention_commands")],
        "checkpointsAfter": checkpoints(&path),
        "digest": sqlite.journal.replay_digest().expect("digest"),
        "projectionDifferences": sqlite.journal.projection_differences().expect("differences").len(),
    });
    Opened { variant, report, state, sqlite }
}

/// Asserts a variant's upgrade request by request, restarts it, and admits
/// live work after it.
fn check(opened: Opened, committed: &Outbox) -> Opened {
    let Opened { variant, mut report, state, mut sqlite } = opened;
    let path = sqlite.store.journal_path();
    let upgraded = outbox_table(&path);
    assert_eq!(upgraded.len(), EXPECTED.len(), "variant {variant}: one row per expected request");
    for (request, scope, reducer1, wanted) in EXPECTED {
        let (attention, now, _) = &upgraded[request];
        assert_eq!(scope_of(&state, attention), scope, "variant {variant}: {request}");
        assert_eq!(committed.get(request).map(|(_, s, _)| s.as_str()), reducer1, "variant {variant}: {request}");
        assert_eq!(now, wanted, "variant {variant}: {request} ({scope})");
        let record = &state.outbox[request];
        assert_eq!(&record.attention_id, attention, "variant {variant}: {request}");
        assert_eq!(serde_json::to_value(record.state).expect("state"), json!(wanted), "variant {variant}: {request}");
    }
    assert_eq!(state.reducer_version, REDUCER_VERSION);
    assert_eq!(report["attentionItems"][0], report["attentionItems"][1], "variant {variant}: no attention item added");
    assert_eq!(report["ownerCommands"], json!([5, 5]), "variant {variant}: every owner command kept");
    assert_eq!(report["projectionDifferences"], 0, "variant {variant}: tables equal the state");
    let after = checkpoints(&path);
    let upgrades: Vec<&Value> = after.iter().filter(|c| c["origin"] == "REDUCER_UPGRADE").collect();
    assert_eq!(upgrades.len(), 1, "variant {variant}: one upgrade checkpoint");
    assert_eq!(upgrades[0]["reducerVersion"], json!(REDUCER_VERSION), "variant {variant}");
    assert_eq!(upgrades[0]["throughCursor"], json!(30), "variant {variant}");
    assert_eq!(after.last(), Some(upgrades[0]), "variant {variant}: the upgrade checkpoint is the newest");
    assert_eq!(report["digest"]["stateSha256"], json!(state_hash(&state)), "variant {variant}: checkpoint + replay");

    // A restart reads the upgrade checkpoint and upgrades nothing again.
    sqlite = sqlite.restart(33).expect("restart");
    assert_eq!(state_hash(sqlite.state()), state_hash(&state), "variant {variant}: restart");
    assert_eq!(checkpoints(&path), after, "variant {variant}: no second upgrade");

    // A live wait after the upgrade is fresh live work: exactly its own
    // intent is PENDING and handed out; the held intents stay held.
    let mut notified = Vec::new();
    for step in live_wait() {
        let Step::Observe(envelope) = step else { continue };
        sqlite.now_ms += 1;
        let admission = EnvelopeAdmission { envelope: &envelope, normalized: normalize_envelope(&envelope) };
        let outcome = sqlite.journal.admit_batch(&[admission], Delivery::Live, sqlite.now_ms).expect("admit");
        notified.extend(outcome.notifications.into_iter().map(|n| n.request_id));
    }
    let live = sqlite.state().clone();
    let fresh: Vec<&String> = live.outbox.keys().filter(|id| !state.outbox.contains_key(*id)).collect();
    assert_eq!(fresh.len(), 1, "variant {variant}: the new wait's intent");
    assert_eq!(notified, vec![fresh[0].clone()], "variant {variant}: only the new intent is handed out");
    assert_eq!(live.outbox[fresh[0]].state, OutboxState::Pending);
    for (request, record) in &state.outbox {
        assert_eq!(live.outbox[request].state, record.state, "variant {variant}: {request} untouched");
    }
    // A restart replays those live entries after the upgrade checkpoint.
    let live_hash = state_hash(&live);
    sqlite = sqlite.restart(34).expect("restart after live");
    assert_eq!(state_hash(sqlite.state()), live_hash, "variant {variant}: replay after the upgrade checkpoint");
    report["restart"] = json!({ "stateSha256": state_hash(&state), "checkpoints": after });
    report["liveAfterUpgrade"] = json!({
        "notified": notified,
        "newIntentState": live.outbox[fresh[0]].state,
        "counts": counts(&outbox_table(&path)),
        "restartStateSha256": live_hash,
    });
    Opened { variant, report, state, sqlite }
}

/// Writes `name` into the directory in `THREADSPACE_WRITE_CHECKPOINT_TAIL`.
fn write(name: &str, report: &Value) {
    if let Ok(dir) = std::env::var("THREADSPACE_WRITE_CHECKPOINT_TAIL") {
        std::fs::write(Path::new(&dir).join(name), serde_json::to_string_pretty(report).expect("json") + "\n")
            .expect("write");
    }
}

#[test]
fn checkpoint_placement_does_not_change_the_upgraded_store() {
    let committed = outbox_table(&fixture("journal.sqlite3"));
    let a = copy(&fixture("journal.sqlite3"), "checkpoint-tail-a");
    let b = copy(&fixture("journal.sqlite3"), "checkpoint-tail-b");
    without_checkpoint_at(&b, 30);
    let c = copy(&fixture("journal-checkpoint-6.sqlite3"), "checkpoint-tail-c");
    without_checkpoint_at(&c, 30);
    let history = history_sha256(&a.journal_path());
    for (variant, store) in [("B", &b), ("C", &c)] {
        assert_eq!(history_sha256(&store.journal_path()), history, "variant {variant} holds A's committed history");
    }
    let newest: Vec<Value> = [&a, &b, &c]
        .iter()
        .map(|s| {
            let last = checkpoints(&s.journal_path()).last().cloned().expect("checkpoint");
            json!([last["reducerVersion"], last["throughCursor"]])
        })
        .collect();
    assert_eq!(newest, vec![json!([1, 30]), json!([1, 0]), json!([1, 6])], "newest reducer-1 checkpoints");

    // Every variant's upgrade is recorded before anything is asserted.
    let opened = [open("A", a, &committed), open("B", b, &committed), open("C", c, &committed)];
    let base = serde_json::to_value(&opened[0].state).expect("state");
    let differences_from_a: Vec<Value> = opened
        .iter()
        .map(|o| {
            let mut diff = Vec::new();
            differences(&base, &serde_json::to_value(&o.state).expect("state"), "", &mut diff);
            json!(diff)
        })
        .collect();
    let mut report = json!({
        "fixtures": {
            "journal.sqlite3": "26ad5fdfcd5c9b33d88b09e7eaeca066e17e68c6c73a14db0e974b51a579454f",
            "journal-checkpoint-6.sqlite3": "3bea2963fc57acb0e746a2b4f5f43873bf88b3c13f0a29d90aa8a918d83e7d9e",
        },
        "reducer1Outbox": counts(&committed),
        "variants": opened.iter().zip(&differences_from_a).map(|(o, d)| {
            let mut r = o.report.clone();
            r["stateDifferencesFromA"] = d.clone();
            r
        }).collect::<Vec<_>>(),
    });
    write("checkpoint-tail.json", &report);

    let checked: Vec<Opened> = opened.into_iter().map(|o| check(o, &committed)).collect();
    for (i, o) in checked.iter().enumerate() {
        report["variants"][i]["restart"] = o.report["restart"].clone();
        report["variants"][i]["liveAfterUpgrade"] = o.report["liveAfterUpgrade"].clone();
    }
    write("checkpoint-tail.json", &report);
    for (i, o) in checked.iter().enumerate().skip(1) {
        let variant = o.variant;
        assert_eq!(differences_from_a[i], json!([]), "variant {variant}: state differs from A");
        for digest in ["stateSha256", "projectionSha256", "tablesSha256", "semanticSha256"] {
            assert_eq!(o.report["digest"][digest], checked[0].report["digest"][digest], "variant {variant}: {digest}");
        }
    }
}

/// Reducer 1 kept only a flag for a wait owner decision's positives without
/// a causal point, which a checkpoint's upgrade reads as one. A decision
/// replayed after that reducer's checkpoint is read the same way, so where
/// the checkpoint sits changes nothing. `journal-unordered-pair.sqlite3`
/// holds one Resolve over P3, U1 and U2: X keeps the checkpoint after it, Y
/// replays it after the cursor-0 checkpoint.
#[test]
fn a_reducer_1_decision_reads_the_same_in_or_after_its_checkpoint() {
    let x = copy(&fixture("journal-unordered-pair.sqlite3"), "unordered-pair-x");
    let y = copy(&fixture("journal-unordered-pair.sqlite3"), "unordered-pair-y");
    without_checkpoint_at(&y, 6);
    assert_eq!(history_sha256(&x.journal_path()), history_sha256(&y.journal_path()), "same committed history");
    let read = |store: TempStore| {
        let sqlite = SqliteRunner::open(store, 32).expect("open and upgrade");
        let state = sqlite.state().clone();
        let decisions: Vec<Value> = state
            .waits
            .values()
            .flat_map(|w| &w.owner_decisions)
            .map(|d| json!({ "commandId": d.command_id, "unordered": d.unordered }))
            .collect();
        let report = json!({
            "decisions": decisions,
            "outbox": counts(&outbox_table(&sqlite.store.journal_path())),
            "digest": sqlite.journal.replay_digest().expect("digest"),
        });
        (state, report)
    };
    let ((x, x_report), (y, y_report)) = (read(x), read(y));
    let mut diff = Vec::new();
    differences(&serde_json::to_value(&x).expect("state"), &serde_json::to_value(&y).expect("state"), "", &mut diff);
    write(
        "unordered-pair.json",
        &json!({
            "fixture": "fixtures/m1/reducer-1-store/journal-unordered-pair.sqlite3",
            "X": x_report,
            "Y": y_report,
            "stateDifferencesXY": diff,
        }),
    );
    assert_eq!(diff, Vec::<String>::new(), "the decision reads the same in or after the checkpoint");
    assert_eq!(x_report["decisions"], json!([{ "commandId": "cmd-resolve-pair", "unordered": 1 }]));
    // Read as covering one of the two, the decision leaves the item
    // actionable: its suppressed intent re-arms, held.
    assert_eq!(x_report["outbox"], json!({ "HELD": 1 }));
}

/// Each attention item's committed resolution time, by its ID.
fn resolution_rows(path: &Path) -> BTreeMap<String, Option<i64>> {
    rusqlite::Connection::open(path)
        .and_then(|c| {
            c.prepare("SELECT id, resolved_at_ms FROM attention_items")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect()
        })
        .expect("attention")
}

const RESOLVED_ITEM: &str = "663fb032-fef5-812b-9104-b15bd1135349";
const OWNER_RESOLVED_AT: i64 = 1_791_000_000_040;

/// Reducer 1 kept the item resolved at the owner's time (cursor 4) when P4,
/// which the owner's Resolve over P3 never covered, arrived after it.
/// Replaying the entries, this reducer reopens the item at P4 and C5
/// resolves it again at cursor 6; the upgrade keeps the committed time,
/// since the item is still resolved. `journal-resolved-at.sqlite3` holds
/// the six entries with reducer-1 checkpoints through cursors 4 and 6:
/// A keeps both, B removes both, C removes the cursor-6 one.
#[test]
fn a_committed_resolution_time_is_kept_wherever_the_checkpoint_sits() {
    let source = fixture("journal-resolved-at.sqlite3");
    let committed = outbox_table(&source);
    assert_eq!(resolution_rows(&source)[RESOLVED_ITEM], Some(OWNER_RESOLVED_AT), "reducer 1 kept the owner's time");
    let a = copy(&source, "resolved-at-a");
    let b = copy(&source, "resolved-at-b");
    without_checkpoint_at(&b, 6);
    without_checkpoint_at(&b, 4);
    let c = copy(&source, "resolved-at-c");
    without_checkpoint_at(&c, 6);
    let history = history_sha256(&a.journal_path());
    for (variant, store) in [("B", &b), ("C", &c)] {
        assert_eq!(history_sha256(&store.journal_path()), history, "variant {variant} holds A's committed history");
    }
    let newest: Vec<Value> = [&a, &b, &c]
        .iter()
        .map(|s| {
            let last = checkpoints(&s.journal_path()).last().cloned().expect("checkpoint");
            json!([last["reducerVersion"], last["throughCursor"]])
        })
        .collect();
    assert_eq!(newest, vec![json!([1, 6]), json!([1, 0]), json!([1, 4])], "newest reducer-1 checkpoints");

    // Every variant's upgrade is recorded before anything is asserted.
    let opened = [open("A", a, &committed), open("B", b, &committed), open("C", c, &committed)];
    let base = serde_json::to_value(&opened[0].state).expect("state");
    let mut variants = Vec::new();
    for o in &opened {
        let item = &o.state.attention[RESOLVED_ITEM];
        let mut diff = Vec::new();
        differences(&base, &serde_json::to_value(&o.state).expect("state"), "", &mut diff);
        let mut report = o.report.clone();
        report["resolvedAtMs"] = json!({
            "state": item.resolved_at_ms,
            "row": resolution_rows(&o.sqlite.store.journal_path())[RESOLVED_ITEM],
        });
        report["attention"] = json!({
            "revision": item.revision,
            "createdCursor": item.created_cursor,
            "resolutions": item.resolutions,
            "resolutionReason": item.resolution_reason,
        });
        report["ownerDecisions"] = json!(o.state.waits.values().flat_map(|w| &w.owner_decisions).collect::<Vec<_>>());
        report["stateDifferencesFromA"] = json!(diff);
        variants.push(report);
    }
    let mut report = json!({
        "fixture": "fixtures/m1/reducer-1-store/journal-resolved-at.sqlite3",
        "reducer1": {
            "resolvedAtMs": OWNER_RESOLVED_AT,
            "outbox": counts(&committed),
        },
        "variants": variants,
    });
    write("resolved-at.json", &report);

    for (i, o) in opened.iter().enumerate() {
        let variant = o.variant;
        let path = o.sqlite.store.journal_path();
        assert_eq!(o.state.attention[RESOLVED_ITEM].resolved_at_ms, Some(OWNER_RESOLVED_AT), "variant {variant}: state");
        assert_eq!(resolution_rows(&path)[RESOLVED_ITEM], Some(OWNER_RESOLVED_AT), "variant {variant}: row");
        assert_eq!(report["variants"][i]["stateDifferencesFromA"], json!([]), "variant {variant}: state differs from A");
        for digest in ["stateSha256", "projectionSha256", "tablesSha256", "semanticSha256"] {
            assert_eq!(o.report["digest"][digest], opened[0].report["digest"][digest], "variant {variant}: {digest}");
        }
        assert_eq!(o.report["projectionDifferences"], 0, "variant {variant}: tables equal the state");
        assert_eq!(o.report["ownerCommands"], json!([1, 1]), "variant {variant}: the owner command kept");
        assert_eq!(o.report["attentionItems"], json!([1, 1]), "variant {variant}: no attention item added");
        // The item is resolved, so its intent stays as reducer 1 left it.
        assert_eq!(outbox_table(&path), committed, "variant {variant}: outbox as committed");
        let decisions = &report["variants"][i]["ownerDecisions"];
        assert_eq!(decisions.as_array().map(Vec::len), Some(1), "variant {variant}: one owner decision");
        assert_eq!(decisions[0]["commandId"], "cmd-resolve-p3", "variant {variant}");
        assert_eq!(decisions[0]["positives"].as_array().map(Vec::len), Some(1), "variant {variant}: covers P3 alone");
        assert_eq!(decisions[0]["unordered"], 0, "variant {variant}");
        let after = checkpoints(&path);
        let upgrades: Vec<&Value> = after.iter().filter(|c| c["origin"] == "REDUCER_UPGRADE").collect();
        assert_eq!(upgrades.len(), 1, "variant {variant}: one upgrade checkpoint");
        assert_eq!(upgrades[0]["throughCursor"], json!(6), "variant {variant}");
        assert_eq!(after.last(), Some(upgrades[0]), "variant {variant}: the upgrade checkpoint is the newest");
    }

    // A restart reads the upgrade checkpoint and upgrades nothing again.
    for (i, o) in opened.into_iter().enumerate() {
        let variant = o.variant;
        let path = o.sqlite.store.journal_path();
        let after = checkpoints(&path);
        let restarted = o.sqlite.restart(33).expect("restart");
        assert_eq!(state_hash(restarted.state()), state_hash(&o.state), "variant {variant}: restart");
        assert_eq!(restarted.state().attention[RESOLVED_ITEM].resolved_at_ms, Some(OWNER_RESOLVED_AT), "variant {variant}");
        assert_eq!(checkpoints(&path), after, "variant {variant}: no second upgrade");
        report["variants"][i]["restart"] = json!({ "stateSha256": state_hash(restarted.state()), "checkpoints": after });
    }
    write("resolved-at.json", &report);
}

/// Each attention item of `journal.sqlite3`: reducer 1's committed
/// resolution time, and the time after the upgrade.
const RESOLUTION_TIMES: [(&str, Option<i64>, Option<i64>); 6] = [
    // Acknowledged by reducer 1; never resolved.
    ("38cc3ee8-5b0f-8db0-b9f9-5009640eba9c", None, None),
    // Resolved by reducer 1; reopened by Q2, Q1 or U2, which no decision
    // covered: no resolution time, whatever reducer 1 committed.
    ("3e47b2b0-a74f-827a-97d8-ce9cae31767e", Some(1_791_000_000_050), None),
    ("663fb032-fef5-812b-9104-b15bd1135349", Some(1_791_000_000_040), None),
    ("7b5efbc8-620e-8af7-b38a-5933656372eb", Some(1_791_000_000_050), None),
    // Snoozed; never resolved.
    ("9c14d766-ad73-835e-ae27-ff21b71c0222", None, None),
    // The superseded episode's item stays resolved at its committed time.
    ("d3561f2e-346b-8bf7-a15f-48d9d2563591", Some(1_791_000_000_070), Some(1_791_000_000_070)),
];

/// The rebase restores a committed resolution time, but the item's current
/// evidence decides whether it is resolved: an item this reducer reopens
/// has no resolution time in any variant, and a committed NULL stays NULL.
#[test]
fn an_item_this_reducer_reopens_keeps_no_resolution_time() {
    let committed = resolution_rows(&fixture("journal.sqlite3"));
    let a = copy(&fixture("journal.sqlite3"), "reopened-a");
    let b = copy(&fixture("journal.sqlite3"), "reopened-b");
    without_checkpoint_at(&b, 30);
    let c = copy(&fixture("journal-checkpoint-6.sqlite3"), "reopened-c");
    without_checkpoint_at(&c, 30);
    let wanted: BTreeMap<String, Option<i64>> = RESOLUTION_TIMES.iter().map(|(id, _, after)| ((*id).to_owned(), *after)).collect();
    for (id, reducer1, _) in RESOLUTION_TIMES {
        assert_eq!(committed[id], reducer1, "reducer 1 committed {id}");
    }
    for (variant, store) in [("A", a), ("B", b), ("C", c)] {
        let sqlite = SqliteRunner::open(store, 32).expect("open and upgrade");
        let state: BTreeMap<String, Option<i64>> =
            sqlite.state().attention.values().map(|a| (a.id.clone(), a.resolved_at_ms)).collect();
        assert_eq!(state, wanted, "variant {variant}: state");
        assert_eq!(resolution_rows(&sqlite.store.journal_path()), wanted, "variant {variant}: rows");
        for (id, item) in &sqlite.state().attention {
            assert_eq!(item.resolved_at_ms.is_some(), !item.resolutions.is_empty(), "variant {variant}: {id}");
        }
    }
}

/// A committed resolution time that is neither an integer nor NULL refuses
/// the upgrade before its transaction: the open fails naming the row, and
/// the store keeps its rows and its reducer-1 checkpoints. The attention
/// table is STRICT, so only a disposable copy whose table definition has
/// lost STRICT can hold such a value.
#[test]
fn a_malformed_committed_resolution_time_refuses_the_upgrade() {
    for (kind, value) in [("text", "'forty'"), ("real", "1791000000040.5"), ("blob", "x'28'")] {
        let store = copy(&fixture("journal-resolved-at.sqlite3"), &format!("resolved-at-{kind}"));
        let path = store.journal_path();
        rusqlite::Connection::open(&path)
            .and_then(|c| {
                c.execute_batch("PRAGMA writable_schema = ON")?;
                c.execute("UPDATE sqlite_schema SET sql = replace(sql, ') STRICT', ')') WHERE name = 'attention_items'", [])
            })
            .expect("drop STRICT");
        let conn = rusqlite::Connection::open(&path).expect("open");
        let sql = format!("UPDATE attention_items SET resolved_at_ms = {value} WHERE id = ?1");
        assert_eq!(conn.execute(&sql, [RESOLVED_ITEM]).expect("malform"), 1);
        let stored: String = conn
            .query_row("SELECT typeof(resolved_at_ms) FROM attention_items WHERE id = ?1", [RESOLVED_ITEM], |r| r.get(0))
            .expect("type");
        assert_eq!(stored, kind);
        drop(conn);
        let (history, before) = (history_sha256(&path), checkpoints(&path));
        let refused = Journal::open_with(
            &path,
            "synthetic-core-1",
            VirtualClock::EPOCH_MS,
            Box::new(SeededAllocator::new(32)),
            false,
        );
        let detail = format!("committed attention row {RESOLVED_ITEM}");
        assert!(
            matches!(&refused, Err(JournalError::Invalid { detail: d }) if *d == detail),
            "{kind}: {:?}",
            refused.err()
        );
        assert_eq!(checkpoints(&path), before, "{kind}: no upgrade checkpoint");
        assert_eq!(history_sha256(&path), history, "{kind}: no row rewritten");
    }
}
