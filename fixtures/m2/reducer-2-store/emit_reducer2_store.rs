//! Emits `fixtures/m2/reducer-2-store/journal*.sqlite3`: stores written by
//! reducer 2 (accepted M1 af9b285da529890bc441ea00f1a92e73e39902a8) holding
//! the histories of the M2 catalog's `attachment-evidence`,
//! `followup-frontier-evidence` and `observer-link-evidence`, built with
//! exactly their builder calls, so the same envelopes reach both reducers.
//! Reducer 2 keeps each of those fields as the latest arrival. It is
//! compiled and run only inside a checkout of that commit, as
//! `tests/synthetic/tests/emit_reducer2_store.rs`:
//!
//! ```text
//! REDUCER2_OUT=<abs path>/journal.sqlite3 \
//!   cargo test -p threadspace-synthetic --test emit_reducer2_store -- --nocapture
//! ```
//!
//! With `REDUCER2_CHECKPOINT_AFTER=<n>` it also checkpoints after the first
//! `n` histories; with `REDUCER2_CHECKPOINT_STEP=<n>` after each history's
//! first `n` steps. Every store ends with a reducer-2 checkpoint.

use serde_json::{Value, json};
use threadspace_contracts::canonical::keys::NativeSessionRef;
use threadspace_synthetic::builder::{Builder, Scenario, process, session};
use threadspace_synthetic::rng::VirtualClock;
use threadspace_synthetic::runner::{Admit, run};
use threadspace_synthetic::scenarios::{CLAUDE_LIKE, WITNESSED};
use threadspace_synthetic::sqlite::{SqliteRunner, TempStore};

const CLAUDE_EXE: &str = "/opt/synthetic/bin/claude";

fn accepted() -> Value {
    json!({ "proof": {
        "engineDispatch": true, "coreSettled": true, "originalOriginProtected": true, "dropped": false
    }})
}

fn complete(b: &mut Builder, s: &NativeSessionRef, turn: &str, reason: &str) -> usize {
    b.obs(Some(s), "turn.complete").turn(turn).payload(json!({ "reason": reason })).push()
}

fn attach_as(
    b: &mut Builder,
    s: &NativeSessionRef,
    activation: &str,
    presence: &str,
    device: u32,
    source: Option<(&str, &str)>,
) -> usize {
    let mut obs = b
        .obs(Some(s), "execution.attach")
        .activation(activation)
        .provider(process(71, 301), CLAUDE_EXE, Some(device))
        .payload(json!({ "mode": "terminal_embedded", "presence": presence, "device": device }));
    if let Some((source, epoch)) = source {
        obs = obs.source(source, epoch).unordered();
    }
    obs.push()
}

fn attachment_evidence() -> Scenario {
    let mut b = Builder::new("attachment-evidence", "evidence-sets");
    let s = session(CLAUDE_LIKE, "sess-attachment-evidence");
    b.obs(Some(&s), "session.start").push();
    attach_as(&mut b, &s, "act-ordered", "LIVE", 7, None);
    attach_as(&mut b, &s, "act-ordered", "DETACHED", 8, None);
    attach_as(&mut b, &s, "act-ordered", "LIVE", 9, None);
    attach_as(&mut b, &s, "act-unordered", "LIVE", 7, None);
    attach_as(&mut b, &s, "act-unordered", "DETACHED", 7, Some(("synthetic.other", "other-1")));
    b.build(|_| Ok(()))
}

fn followup_frontier_evidence() -> Scenario {
    let mut b = Builder::new("followup-frontier-evidence", "evidence-sets");
    for (native, prompt) in [
        ("sess-frontier-rejected", "p-rejected"),
        ("sess-frontier-origin", "p-origin"),
        ("sess-frontier-control", "p-control"),
    ] {
        let s = session(WITNESSED, native);
        b.obs(Some(&s), "session.start").push();
        b.obs(Some(&s), "turn.start").turn("t1").push();
        b.obs(Some(&s), "notify.output").turn("t1").payload(json!({ "nativeKey": "t1-output" })).push();
        complete(&mut b, &s, "t1", "answer");
        b.obs(Some(&s), "prompt.submit").prompt(prompt).payload(json!({ "origin": "HUMAN_COMPOSER" })).push();
        b.obs(Some(&s), "prompt.accepted").prompt(prompt).payload(accepted()).push();
        match native {
            "sess-frontier-rejected" => {
                b.obs(Some(&s), "prompt.rejected")
                    .prompt(prompt)
                    .payload(json!({ "reason": "BLOCKED_BY_HOOK" }))
                    .source("synthetic.hook", "hook-1")
                    .unordered()
                    .push();
            }
            "sess-frontier-origin" => {
                b.obs(Some(&s), "prompt.submit")
                    .prompt(prompt)
                    .payload(json!({ "origin": "SCHEDULED" }))
                    .source("synthetic.hook", "hook-1")
                    .unordered()
                    .push();
            }
            _ => {}
        }
    }
    b.build(|_| Ok(()))
}

fn observer_link_evidence() -> Scenario {
    let mut b = Builder::new("observer-link-evidence", "evidence-sets");
    let ordered = session(CLAUDE_LIKE, "sess-link-ordered");
    b.obs(Some(&ordered), "session.start").push();
    b.obs(Some(&ordered), "observer.link").payload(json!({ "link": "CURRENT" })).push();
    b.obs(Some(&ordered), "observer.link").payload(json!({ "link": "STALE" })).push();
    b.obs(Some(&ordered), "observer.link").payload(json!({ "link": "CURRENT" })).push();
    let unordered = session(CLAUDE_LIKE, "sess-link-unordered");
    b.obs(Some(&unordered), "session.start").push();
    b.obs(Some(&unordered), "observer.link").payload(json!({ "link": "CURRENT" })).push();
    b.obs(Some(&unordered), "observer.link")
        .payload(json!({ "link": "DISCONNECTED" }))
        .source("synthetic.observer", "observer-2")
        .unordered()
        .push();
    b.build(|_| Ok(()))
}

#[test]
fn emit() {
    let out = std::env::var("REDUCER2_OUT").expect("REDUCER2_OUT");
    let after: Option<usize> = std::env::var("REDUCER2_CHECKPOINT_AFTER").ok().map(|n| n.parse().expect("count"));
    let step: Option<usize> = std::env::var("REDUCER2_CHECKPOINT_STEP").ok().map(|n| n.parse().expect("count"));
    let mut sqlite = SqliteRunner::open(TempStore::new("reducer2-emit"), 31).expect("open");
    for (done, scenario) in [attachment_evidence(), followup_frontier_evidence(), observer_link_evidence()]
        .into_iter()
        .enumerate()
    {
        let order: Vec<usize> = (0..scenario.steps.len()).collect();
        let report = match step {
            Some(n) => {
                run(&scenario, &order[..n], &mut sqlite);
                sqlite.journal.checkpoint("FIXTURE", VirtualClock::EPOCH_MS + 250_000).expect("checkpoint");
                run(&scenario, &order[n..], &mut sqlite)
            }
            None => run(&scenario, &order, &mut sqlite),
        };
        assert!(report.owner_failures.is_empty(), "{}: {:?}", scenario.name, report.owner_failures);
        if after == Some(done + 1) {
            sqlite.journal.checkpoint("FIXTURE", VirtualClock::EPOCH_MS + 500_000).expect("checkpoint");
        }
    }
    sqlite.journal.checkpoint("FIXTURE", VirtualClock::EPOCH_MS + 1_000_000).expect("checkpoint");
    let state = sqlite.state().clone();
    let executions: Vec<Value> = state
        .executions
        .values()
        .map(|e| json!({ "activation": e.activation_ref, "attached": e.attached, "device": e.controlling_device }))
        .collect();
    let sessions: Vec<Value> = state
        .sessions
        .values()
        .map(|s| json!({ "session": s.native_session_id, "link": s.link, "observation": s.observation }))
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "reducerVersion": state.reducer_version,
            "executions": executions,
            "sessions": sessions,
            "frontiers": state.frontiers.len(),
        }))
        .expect("json")
    );
    rusqlite::Connection::open(sqlite.store.journal_path())
        .and_then(|c| c.execute("VACUUM INTO ?1", [out.as_str()]))
        .expect("export");
}
