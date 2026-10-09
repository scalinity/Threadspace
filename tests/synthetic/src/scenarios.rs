//! The M1 synthetic scenario catalog (SPEC §21.3; MILESTONES M1 "Synthetic
//! scenarios"). Each scenario is a native-shaped history in causal order and
//! an expectation over the converged canonical state. No model calls.

use serde_json::{Value, json};
use threadspace_contracts::canonical::command::OwnerAction;
use threadspace_contracts::canonical::keys::NativeSessionRef;
use threadspace_contracts::canonical::records::{CanonicalState, ObserverTier, OutboxState, ResolutionKind};
use threadspace_contracts::projection::{
    AttentionCategory, ExecutionPresence, ObservationState, TurnState,
};

use crate::builder::{Builder, DEFAULT_SOURCE, Scenario, Target, process, session};
use crate::view::{View, resolved_by};

macro_rules! ensure {
    ($cond:expr, $($arg:tt)*) => {
        if !$cond {
            return Err(format!($($arg)*));
        }
    };
}

/// A synthetic profile with Claude 2.1.291's capabilities (D-0005).
pub const CLAUDE_LIKE: &str = "claude-like";
/// A synthetic profile with a qualified original-order witness.
pub const WITNESSED: &str = "witnessed-1";

const CLAUDE_EXE: &str = "/opt/synthetic/bin/claude";

fn accepted() -> Value {
    json!({ "proof": {
        "engineDispatch": true, "coreSettled": true, "originalOriginProtected": true, "dropped": false
    }})
}

fn tty_surface(locator: &str, device: u32, generation: &str) -> Value {
    json!({
        "surfaceKind": "terminal.app",
        "appGeneration": "terminal-gen-1",
        "locator": locator,
        "deviceNumber": device,
        "surfaceGeneration": generation,
    })
}

fn attach(b: &mut Builder, s: &NativeSessionRef, activation: &str, pid: u32, start: u64, device: u32) -> usize {
    b.obs(Some(s), "execution.attach")
        .activation(activation)
        .provider(process(pid, start), CLAUDE_EXE, Some(device))
        .payload(json!({ "mode": "terminal_embedded", "presence": "LIVE", "device": device }))
        .push()
}

fn bind(b: &mut Builder, s: &NativeSessionRef, activation: &str, locator: &str, device: u32, generation: &str) -> usize {
    b.obs(Some(s), "surface.bind")
        .activation(activation)
        .payload(json!({
            "surface": tty_surface(locator, device, generation),
            "method": "NATIVE_INVENTORY",
            "executable": CLAUDE_EXE,
            "windowHint": 1, "tabHint": 1,
        }))
        .push()
}

fn tool(b: &mut Builder, s: &NativeSessionRef, turn: &str, id: &str, phase: &str) -> usize {
    let mut payload = json!({ "phase": phase, "toolCategory": "bash" });
    if phase == "finished" {
        payload["result"] = json!("SUCCESS");
    }
    b.obs(Some(s), "tool.call").turn(turn).occurrence(id).payload(payload).push()
}

fn complete(b: &mut Builder, s: &NativeSessionRef, turn: &str, reason: &str) -> usize {
    b.obs(Some(s), "turn.complete").turn(turn).payload(json!({ "reason": reason })).push()
}

fn turn_state(v: &View<'_>, session: &str, turn: &str) -> Result<TurnState, String> {
    Ok(v.turn(session, None, turn).ok_or(format!("turn {turn} missing"))?.state.clone())
}

// ------------------------------------------------------------------ completion

pub fn normal_session() -> Scenario {
    let mut b = Builder::new("normal-session", "completion");
    let s = session(CLAUDE_LIKE, "sess-normal");
    b.obs(Some(&s), "session.start").payload(json!({ "source": "startup", "displayName": "Normal worker" })).push();
    attach(&mut b, &s, "act-1", 4100, 1000, 16_777_220);
    bind(&mut b, &s, "act-1", "/dev/ttys004", 16_777_220, "4100:1000");
    b.obs(Some(&s), "prompt.submit").prompt("p1").payload(json!({ "origin": "HUMAN_COMPOSER" })).push();
    b.obs(Some(&s), "prompt.accepted").prompt("p1").payload(accepted()).push();
    b.obs(Some(&s), "turn.start").turn("t1").prompt("p1").push();
    tool(&mut b, &s, "t1", "tool-1", "proposed");
    tool(&mut b, &s, "t1", "tool-1", "started");
    b.obs(Some(&s), "tool.check").turn("t1").occurrence("tool-1").payload(json!({ "toolCategory": "bash" })).push();
    tool(&mut b, &s, "t1", "tool-1", "finished");
    b.obs(Some(&s), "turn.step").turn("t1").push();
    b.obs(Some(&s), "notify.output").turn("t1").push();
    complete(&mut b, &s, "t1", "answer");
    b.obs(Some(&s), "Stop").turn("t1").payload(json!({ "stopHookActive": false })).push();
    b.build(|state| {
        let v = View::new(state);
        ensure!(v.sessions_named("sess-normal") == 1, "one session");
        ensure!(turn_state(&v, "sess-normal", "t1")? == TurnState::Completed, "t1 completed");
        let turn = v.turn("sess-normal", None, "t1").ok_or("t1")?;
        let item = v.output_item(turn).ok_or("output item")?;
        ensure!(item.category == AttentionCategory::TurnComplete, "TURN_COMPLETE item");
        ensure!(!item.resolved(), "Claude-like output stays unresolved (D-0005)");
        let execution = v.execution("sess-normal", "act-1").ok_or("act-1")?;
        ensure!(execution.presence == ExecutionPresence::Live, "Stop/completion never end the execution");
        ensure!(v.bindings(execution).iter().filter(|b| b.valid).count() == 1, "one valid binding");
        ensure!(v.session("sess-normal").ok_or("s")?.execution_presence == ExecutionPresence::Live, "session retained live");
        let input = state.inputs.values().find(|i| i.native_key == "p1").ok_or("p1")?;
        ensure!(input.started_turns.len() == 1, "p1 started t1");
        Ok(())
    })
}

pub fn parallel_tools() -> Scenario {
    let mut b = Builder::new("parallel-tools", "completion");
    let s = session(CLAUDE_LIKE, "sess-parallel");
    b.obs(Some(&s), "session.start").push();
    b.obs(Some(&s), "turn.start").turn("t1").push();
    for id in ["tool-a", "tool-b", "tool-c"] {
        tool(&mut b, &s, "t1", id, "proposed");
    }
    for id in ["tool-b", "tool-a", "tool-c"] {
        tool(&mut b, &s, "t1", id, "started");
    }
    tool(&mut b, &s, "t1", "tool-a", "finished");
    tool(&mut b, &s, "t1", "tool-b", "finished");
    complete(&mut b, &s, "t1", "answer");
    // A delayed tool callback after the terminal outcome (INV-08).
    tool(&mut b, &s, "t1", "tool-c", "finished");
    b.obs(Some(&s), "turn.step").turn("t1").push();
    b.build(|state| {
        let v = View::new(state);
        ensure!(turn_state(&v, "sess-parallel", "t1")? == TurnState::Completed, "late tool activity cannot regress COMPLETED");
        ensure!(state.activities.len() == 3, "three activities, got {}", state.activities.len());
        ensure!(state.activities.values().all(|a| a.started && !a.finished.is_empty()), "all finished");
        Ok(())
    })
}

pub fn outcomes() -> Scenario {
    let mut b = Builder::new("outcomes", "completion");
    let s = session(CLAUDE_LIKE, "sess-outcomes");
    b.obs(Some(&s), "session.start").push();
    for (turn, reason) in [("t-int", "aborted"), ("t-ref", "refusal"), ("t-fail", "error")] {
        b.obs(Some(&s), "turn.start").turn(turn).push();
        tool(&mut b, &s, turn, &format!("{turn}-tool"), "started");
        complete(&mut b, &s, turn, reason);
    }
    b.build(|state| {
        let v = View::new(state);
        ensure!(turn_state(&v, "sess-outcomes", "t-int")? == TurnState::Interrupted, "interruption");
        ensure!(turn_state(&v, "sess-outcomes", "t-ref")? == TurnState::Refused, "refusal");
        ensure!(turn_state(&v, "sess-outcomes", "t-fail")? == TurnState::Failed, "failure");
        let int = v.turn("sess-outcomes", None, "t-int").ok_or("t-int")?;
        ensure!(v.output_item(int).is_none(), "an interruption alone creates no owner item");
        for turn in ["t-ref", "t-fail"] {
            let t = v.turn("sess-outcomes", None, turn).ok_or("turn")?;
            let item = v.output_item(t).ok_or("error item")?;
            ensure!(item.category == AttentionCategory::Error && item.priority == 90, "{turn} ERROR item");
        }
        Ok(())
    })
}

pub fn latest_turn_outcome() -> Scenario {
    let mut b = Builder::new("latest-turn-outcome", "completion");
    let s = session(CLAUDE_LIKE, "sess-latest");
    b.obs(Some(&s), "session.start").sequence(1).push();
    // Captured order: t1 completes (2, 3), then t2 is interrupted (4, 5).
    // Built (the reference delivery) t2 first: t1 arrives late.
    b.obs(Some(&s), "turn.start").turn("t2").sequence(4).push();
    b.obs(Some(&s), "turn.complete").turn("t2").sequence(5).payload(json!({ "reason": "aborted" })).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(2).push();
    b.obs(Some(&s), "turn.complete").turn("t1").sequence(3).payload(json!({ "reason": "answer" })).push();
    b.build(|state| {
        let v = View::new(state);
        let session = v.session("sess-latest").ok_or("session")?;
        ensure!(session.turn_state == TurnState::Interrupted, "the causally latest turn sets the session's state, got {:?}", session.turn_state);
        Ok(())
    })
}

// ------------------------------------------------------------------ duplicates

pub fn duplicate_deliveries() -> Scenario {
    let mut b = Builder::new("duplicate-deliveries", "duplicates");
    let s = session(CLAUDE_LIKE, "sess-dup");
    let start = b.obs(Some(&s), "session.start").push();
    let turn = b.obs(Some(&s), "turn.start").turn("t1").push();
    let done = complete(&mut b, &s, "t1", "answer");
    for of in [start, turn, done] {
        for _ in 0..9 {
            b.duplicate(of);
        }
    }
    b.build(|state| {
        let v = View::new(state);
        ensure!(v.sessions_named("sess-dup") == 1, "ten deliveries: one session");
        ensure!(state.turns.len() == 1, "one turn");
        ensure!(state.attention.len() == 1, "one output item");
        ensure!(turn_state(&v, "sess-dup", "t1")? == TurnState::Completed, "completed");
        Ok(())
    })
}

// ------------------------------------------------------------------ actors

pub fn child_actors() -> Scenario {
    let mut b = Builder::new("child-actors", "actor-events");
    let s = session(CLAUDE_LIKE, "sess-actors");
    b.obs(Some(&s), "session.start").push();
    b.obs(Some(&s), "turn.start").turn("t1").push();
    b.obs(Some(&s), "agent.spawn").agent("a1").payload(json!({ "agentType": "general" })).push();
    b.obs(Some(&s), "agent.spawn").agent("a2").payload(json!({ "parentAgentId": "a1", "agentType": "explorer" })).push();
    b.obs(Some(&s), "turn.start").agent("a1").turn("a1-t1").push();
    b.obs(Some(&s), "turn.start").agent("a2").turn("a2-t1").push();
    b.obs(Some(&s), "notify.output").agent("a2").turn("a2-t1").push();
    b.obs(Some(&s), "turn.complete").agent("a2").turn("a2-t1").payload(json!({ "reason": "answer" })).push();
    b.obs(Some(&s), "agent.end").agent("a2").push();
    b.obs(Some(&s), "turn.complete").agent("a1").turn("a1-t1").payload(json!({ "reason": "error" })).push();
    complete(&mut b, &s, "t1", "answer");
    b.build(|state| {
        let v = View::new(state);
        ensure!(state.actors.len() == 3, "principal + two children, got {}", state.actors.len());
        ensure!(state.relations.len() == 2, "two immediate-parent relations");
        for (agent, turn, outcome) in [("a1", "a1-t1", TurnState::Failed), ("a2", "a2-t1", TurnState::Completed)] {
            let t = v.turn("sess-actors", Some(agent), turn).ok_or("child turn")?;
            ensure!(t.state == outcome, "{turn} outcome");
            ensure!(!t.owner_facing && v.output_item(t).is_none(), "parent-owned child output creates no owner item");
        }
        let root = v.turn("sess-actors", None, "t1").ok_or("t1")?;
        ensure!(v.output_item(root).is_some(), "principal output item");
        ensure!(state.attention.len() == 1, "only the principal's item");
        Ok(())
    })
}

pub fn shared_native_turn_id() -> Scenario {
    let mut b = Builder::new("shared-native-turn-id", "actor-events");
    let s = session(CLAUDE_LIKE, "sess-shared-turn");
    b.obs(Some(&s), "session.start").push();
    b.obs(Some(&s), "turn.start").turn("t1").push();
    b.obs(Some(&s), "agent.spawn").agent("a1").payload(json!({ "agentType": "general" })).push();
    // Native turn IDs are scoped to their actor: the subagent's "t1" is its own.
    b.obs(Some(&s), "turn.start").agent("a1").turn("t1").push();
    b.obs(Some(&s), "turn.complete").agent("a1").turn("t1").payload(json!({ "reason": "error" })).push();
    complete(&mut b, &s, "t1", "answer");
    b.build(|state| {
        let v = View::new(state);
        ensure!(state.turns.len() == 2, "one turn per actor, got {}", state.turns.len());
        ensure!(turn_state(&v, "sess-shared-turn", "t1")? == TurnState::Completed, "principal t1");
        let child = v.turn("sess-shared-turn", Some("a1"), "t1").ok_or("child t1")?;
        ensure!(child.state == TurnState::Failed, "child t1");
        Ok(())
    })
}

// ------------------------------------------------------------------ waits

pub fn waiting() -> Scenario {
    let mut b = Builder::new("waiting", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-wait");
    b.obs(Some(&s), "session.start").push();
    b.obs(Some(&s), "turn.start").turn("t1").push();
    b.obs(Some(&s), "wait").turn("t1").payload(json!({ "category": "INPUT", "signal": "POSITIVE" })).push();
    b.obs(Some(&s), "wait").turn("t1").payload(json!({ "category": "INPUT", "signal": "CLEARED" })).push();
    b.obs(Some(&s), "wait").turn("t1").payload(json!({ "category": "APPROVAL", "signal": "POSITIVE", "requestId": "req-1" })).push();
    b.obs(Some(&s), "request.resolved").turn("t1").payload(json!({ "requestId": "req-1" })).push();
    b.obs(Some(&s), "wait").turn("t1").payload(json!({ "category": "APPROVAL", "signal": "POSITIVE", "requestId": "req-2" })).push();
    b.build(|state| {
        let v = View::new(state);
        let waits = v.wait_items("sess-wait");
        ensure!(waits.len() == 1, "one input episode item");
        ensure!(resolved_by(waits[0], ResolutionKind::WaitEnded), "input wait ended");
        let req1 = v.request_item("sess-wait", "req-1").ok_or("req-1 item")?;
        ensure!(resolved_by(req1, ResolutionKind::RequestResolved), "req-1 resolved by its own ID");
        let req2 = v.request_item("sess-wait", "req-2").ok_or("req-2 item")?;
        ensure!(!req2.resolved() && req2.category == AttentionCategory::ApprovalRequired, "req-2 open approval");
        ensure!(turn_state(&v, "sess-wait", "t1")? == TurnState::Waiting, "t1 waiting on req-2");
        Ok(())
    })
}

pub fn delayed_positive_wait() -> Scenario {
    let mut b = Builder::new("delayed-positive-wait", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-delayed-wait");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(2).push();
    // Captured order: positive (5) then clear (7). Built (and so, in the
    // reference delivery) clear first: the positive arrives late.
    b.obs(Some(&s), "wait").turn("t1").sequence(7).payload(json!({ "category": "INPUT", "signal": "CLEARED" })).push();
    b.obs(Some(&s), "wait").turn("t1").sequence(5).payload(json!({ "category": "INPUT", "signal": "POSITIVE" })).push();
    b.obs(Some(&s), "turn.step").turn("t1").sequence(8).push();
    b.build(|state| {
        let v = View::new(state);
        let waits = v.wait_items("sess-delayed-wait");
        ensure!(waits.len() == 1 && resolved_by(waits[0], ResolutionKind::WaitEnded), "historical positive cannot reopen attention");
        ensure!(turn_state(&v, "sess-delayed-wait", "t1")? == TurnState::Working, "not WAITING");
        ensure!(state.outbox.values().all(|o| o.state != threadspace_contracts::canonical::records::OutboxState::Pending), "no live banner for a historical wait");
        Ok(())
    })
}

pub fn delayed_clear_wait() -> Scenario {
    let mut b = Builder::new("delayed-clear-wait", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-delayed-clear");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(2).push();
    // Captured order: clear (3), positive (5), clear (7). Built (and so, in the
    // reference delivery) positive first: the earlier clear arrives late and
    // moves the positive into the next episode.
    b.obs(Some(&s), "wait").turn("t1").sequence(5).payload(json!({ "category": "INPUT", "signal": "POSITIVE" })).push();
    b.obs(Some(&s), "wait").turn("t1").sequence(3).payload(json!({ "category": "INPUT", "signal": "CLEARED" })).push();
    b.obs(Some(&s), "wait").turn("t1").sequence(7).payload(json!({ "category": "INPUT", "signal": "CLEARED" })).push();
    b.obs(Some(&s), "turn.step").turn("t1").sequence(8).push();
    b.build(|state| {
        let v = View::new(state);
        let waits = v.wait_items("sess-delayed-clear");
        ensure!(!waits.is_empty(), "the wait raised an item");
        ensure!(waits.iter().all(|w| resolved_by(w, ResolutionKind::WaitEnded)), "no wait item stays open after the last clear");
        ensure!(turn_state(&v, "sess-delayed-clear", "t1")? == TurnState::Working, "not WAITING");
        Ok(())
    })
}

fn input_wait(b: &mut Builder, s: &NativeSessionRef, turn: Option<&str>, sequence: u64, signal: &str) -> usize {
    let obs = b.obs(Some(s), "wait").sequence(sequence).payload(json!({ "category": "INPUT", "signal": signal }));
    match turn {
        Some(turn) => obs.turn(turn).push(),
        None => obs.push(),
    }
}

/// Every unresolved, unacknowledged wait item has a currently eligible
/// intent, and no resolved one does.
fn wait_eligibility_matches(state: &CanonicalState, v: &View<'_>, session: &str) -> Result<usize, String> {
    let mut open = 0;
    for item in v.wait_items(session) {
        let eligible = state.outbox.values().any(|o| {
            o.attention_id == item.id && matches!(o.state, OutboxState::Pending | OutboxState::Held)
        });
        let unresolved = !item.resolved() && !item.acknowledged();
        ensure!(eligible == unresolved, "item eligible={eligible} but unresolved={unresolved}");
        open += usize::from(unresolved);
    }
    Ok(open)
}

/// P10, C5 (a comparable clear before it, arriving late) and Q (a positive
/// from another source epoch): the episode C5 empties reappears with Q,
/// and its item is notifiable again.
pub fn wait_reappears() -> Scenario {
    let mut b = Builder::new("wait-reappears", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-reappear");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(2).push();
    input_wait(&mut b, &s, Some("t1"), 10, "POSITIVE");
    input_wait(&mut b, &s, Some("t1"), 5, "CLEARED");
    b.obs(Some(&s), "wait").turn("t1").source(DEFAULT_SOURCE, "mod-epoch-2").sequence(1)
        .payload(json!({ "category": "INPUT", "signal": "POSITIVE" })).push();
    b.build(|state| {
        let v = View::new(state);
        ensure!(wait_eligibility_matches(state, &v, "sess-reappear")? == 2, "P10's and Q's waits are both open");
        ensure!(turn_state(&v, "sess-reappear", "t1")? == TurnState::Waiting, "WAITING");
        Ok(())
    })
}

/// P2, C3 (which clears it) and Q (incomparable with C3): Q keeps the first
/// episode open and uncertain, and it stays notifiable whatever arrived first.
pub fn wait_uncertain_reopens() -> Scenario {
    let mut b = Builder::new("wait-uncertain-reopens", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-uncertain");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(1).push();
    input_wait(&mut b, &s, Some("t1"), 2, "POSITIVE");
    input_wait(&mut b, &s, Some("t1"), 3, "CLEARED");
    b.obs(Some(&s), "wait").turn("t1").source(DEFAULT_SOURCE, "mod-epoch-2").sequence(1)
        .payload(json!({ "category": "INPUT", "signal": "POSITIVE" })).push();
    b.build(|state| {
        let v = View::new(state);
        ensure!(wait_eligibility_matches(state, &v, "sess-uncertain")? == 1, "one open, uncertain wait");
        Ok(())
    })
}

/// P10, the owner marks its item handled, then C5 arrives late and moves P10
/// into the next episode: the owner's decision still governs P10.
pub fn wait_owner_resolution_kept() -> Scenario {
    let mut b = Builder::new("wait-owner-resolution-kept", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-owner-wait");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(2).push();
    let p10 = input_wait(&mut b, &s, Some("t1"), 10, "POSITIVE");
    let target = Target::Wait { session: s.clone(), turn: Some("t1".into()), category: "INPUT".into(), witness: 10 };
    b.owner("cmd-wait-resolve", target, OwnerAction::Resolve { reason: "answered in the terminal".into() }, &[p10]);
    input_wait(&mut b, &s, Some("t1"), 5, "CLEARED");
    b.build(|state| {
        let v = View::new(state);
        ensure!(wait_eligibility_matches(state, &v, "sess-owner-wait")? == 0, "no wait item reopens");
        ensure!(v.wait_items("sess-owner-wait").iter().any(|i| resolved_by(i, ResolutionKind::Owner)), "the owner's resolution governs P10");
        Ok(())
    })
}

/// An owner decision covers the condition it was made on, not a new wait:
/// after a clear, a new positive is open again.
pub fn wait_new_after_resolution() -> Scenario {
    let mut b = Builder::new("wait-new-after-resolution", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-new-wait");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(2).push();
    let p10 = input_wait(&mut b, &s, Some("t1"), 10, "POSITIVE");
    let target = Target::Wait { session: s.clone(), turn: Some("t1".into()), category: "INPUT".into(), witness: 10 };
    let resolve = b.owner("cmd-wait-handled", target, OwnerAction::Resolve { reason: "handled".into() }, &[p10]);
    let clear = input_wait(&mut b, &s, Some("t1"), 12, "CLEARED");
    let p15 = input_wait(&mut b, &s, Some("t1"), 15, "POSITIVE");
    // P15 follows the owner's decision: the owner could not have seen it.
    b.before(resolve, p15);
    b.before(resolve, clear);
    b.build(|state| {
        let v = View::new(state);
        ensure!(wait_eligibility_matches(state, &v, "sess-new-wait")? == 1, "P15's wait is open");
        ensure!(turn_state(&v, "sess-new-wait", "t1")? == TurnState::Waiting, "WAITING again");
        Ok(())
    })
}

/// t1 waits, clears and completes; t2 then waits. Each wait belongs to its
/// own turn, whatever arrived first.
pub fn wait_turn_ownership() -> Scenario {
    let mut b = Builder::new("wait-turn-ownership", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-turn-wait");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(2).push();
    input_wait(&mut b, &s, Some("t1"), 3, "POSITIVE");
    input_wait(&mut b, &s, Some("t1"), 4, "CLEARED");
    b.obs(Some(&s), "turn.complete").turn("t1").sequence(5).payload(json!({ "reason": "answer" })).push();
    b.obs(Some(&s), "turn.start").turn("t2").sequence(6).push();
    input_wait(&mut b, &s, Some("t2"), 7, "POSITIVE");
    b.build(|state| {
        let v = View::new(state);
        ensure!(turn_state(&v, "sess-turn-wait", "t1")? == TurnState::Completed, "t1 COMPLETED");
        ensure!(turn_state(&v, "sess-turn-wait", "t2")? == TurnState::Waiting, "t2 WAITING");
        let t2 = v.turn("sess-turn-wait", None, "t2").ok_or("t2")?;
        let open: Vec<_> = v.wait_items("sess-turn-wait").into_iter().filter(|i| !i.resolved()).collect();
        ensure!(open.len() == 1 && open[0].turn_id.as_deref() == Some(t2.id.as_str()), "the open wait is t2's");
        Ok(())
    })
}

/// A wait with no turn identity (an inventory wait) stays session-scoped:
/// it is never guessed onto the running turn.
pub fn wait_session_scoped() -> Scenario {
    let mut b = Builder::new("wait-session-scoped", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-scoped-wait");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(2).push();
    input_wait(&mut b, &s, None, 3, "POSITIVE");
    b.build(|state| {
        let v = View::new(state);
        ensure!(turn_state(&v, "sess-scoped-wait", "t1")? == TurnState::Working, "t1 not WAITING");
        let items = v.wait_items("sess-scoped-wait");
        ensure!(items.len() == 1 && items[0].turn_id.is_none(), "a session-scoped item");
        ensure!(wait_eligibility_matches(state, &v, "sess-scoped-wait")? == 1, "open and notifiable");
        Ok(())
    })
}

pub fn wait_generations() -> Scenario {
    let mut b = Builder::new("wait-generations", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-gen");
    b.obs(Some(&s), "session.start").push();
    b.obs(Some(&s), "wait").payload(json!({ "category": "APPROVAL", "signal": "POSITIVE", "generation": "g1" })).push();
    b.obs(Some(&s), "wait").payload(json!({ "category": "APPROVAL", "signal": "POSITIVE", "generation": "g2" })).push();
    b.obs(Some(&s), "wait").payload(json!({ "category": "APPROVAL", "signal": "CLEARED", "generation": "g1" })).push();
    // An incomparable clear (another source epoch) never clears automatically.
    b.obs(Some(&s), "wait").source("synthetic.inventory", "inv-epoch-1").payload(json!({ "category": "INPUT", "signal": "POSITIVE" })).push();
    b.obs(Some(&s), "wait").payload(json!({ "category": "INPUT", "signal": "CLEARED" })).push();
    b.build(|state| {
        let v = View::new(state);
        let items = v.wait_items("sess-gen");
        ensure!(items.len() == 3, "g1, g2 and input episodes, got {}", items.len());
        let ended = items.iter().filter(|i| resolved_by(i, ResolutionKind::WaitEnded)).count();
        ensure!(ended == 1, "only g1 ended: a clear cannot resolve another generation or incomparable evidence");
        let uncertain = state.waits.values().flat_map(|w| &w.episodes).filter(|e| e.uncertain).count();
        ensure!(uncertain == 1, "incomparable positive stays uncertain");
        Ok(())
    })
}

/// The decisions a wait scope keeps, as (command, sequences of the
/// positives it covered).
fn decisions(state: &CanonicalState, session: &str) -> Vec<(String, Vec<String>)> {
    let v = View::new(state);
    let Some(session) = v.session(session) else { return Vec::new() };
    let mut out: Vec<(String, Vec<String>)> = state
        .waits
        .values()
        .filter(|w| w.session_id == session.id)
        .flat_map(|w| &w.owner_decisions)
        .map(|d| (d.command_id.clone(), d.positives.iter().filter_map(|p| p.sequence.clone()).collect()))
        .collect();
    out.sort();
    out
}

fn other_epoch_wait(b: &mut Builder, s: &NativeSessionRef, turn: &str, sequence: u64, signal: &str) -> usize {
    b.obs(Some(s), "wait").turn(turn).source(DEFAULT_SOURCE, "mod-epoch-2").sequence(sequence)
        .payload(json!({ "category": "INPUT", "signal": signal })).push()
}

fn wait_target(s: &NativeSessionRef, turn: Option<&str>, witness: u64) -> Target {
    Target::Wait { session: s.clone(), turn: turn.map(str::to_owned), category: "INPUT".into(), witness }
}

/// P2 (epoch A), the owner resolves it, C3 (A) clears it, then Q1 (epoch B,
/// captured after the decision) lands in the same episode, incomparable with
/// C3. The resolution covered P alone: Q, which the owner never handled,
/// keeps the item open and notifiable.
pub fn wait_owner_partial_coverage() -> Scenario {
    let mut b = Builder::new("wait-owner-partial-coverage", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-partial");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(1).push();
    let p2 = input_wait(&mut b, &s, Some("t1"), 2, "POSITIVE");
    let resolve = b.owner("cmd-resolve-p", wait_target(&s, Some("t1"), 2), OwnerAction::Resolve { reason: "handled P".into() }, &[p2]);
    let c3 = input_wait(&mut b, &s, Some("t1"), 3, "CLEARED");
    let q1 = other_epoch_wait(&mut b, &s, "t1", 1, "POSITIVE");
    b.before(resolve, c3);
    b.before(resolve, q1);
    b.build(|state| {
        let v = View::new(state);
        ensure!(decisions(state, "sess-partial") == vec![("cmd-resolve-p".into(), vec!["2".into()])], "the resolution covers P2 alone");
        let items = v.wait_items("sess-partial");
        ensure!(items.len() == 1 && !items[0].resolved(), "P and Q share one item, open for Q");
        ensure!(wait_eligibility_matches(state, &v, "sess-partial")? == 1, "Q's wait is notifiable");
        ensure!(turn_state(&v, "sess-partial", "t1")? == TurnState::Waiting, "WAITING on Q");
        Ok(())
    })
}

/// P10 (A), the owner resolves it, C5 (A, earlier) moves it to episode 1,
/// Q2 (B) lands in episode 0, then D1 (B, before Q2) arrives late and merges
/// Q2 into P10's episode. P10's resolution stays with P10: the merged item
/// is open for Q2, and no second decision appears.
pub fn wait_owner_merge_keeps_coverage() -> Scenario {
    let mut b = Builder::new("wait-owner-merge-keeps-coverage", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-merge");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(2).push();
    let p10 = input_wait(&mut b, &s, Some("t1"), 10, "POSITIVE");
    let resolve = b.owner("cmd-resolve-p10", wait_target(&s, Some("t1"), 10), OwnerAction::Resolve { reason: "handled P10".into() }, &[p10]);
    input_wait(&mut b, &s, Some("t1"), 5, "CLEARED");
    let q2 = other_epoch_wait(&mut b, &s, "t1", 2, "POSITIVE");
    other_epoch_wait(&mut b, &s, "t1", 1, "CLEARED");
    b.before(resolve, q2);
    b.build(|state| {
        let v = View::new(state);
        ensure!(decisions(state, "sess-merge") == vec![("cmd-resolve-p10".into(), vec!["10".into()])], "one decision, covering P10 alone");
        ensure!(state.commands.len() == 1, "no owner command manufactured");
        let open: Vec<_> = v.wait_items("sess-merge").into_iter().filter(|i| !i.resolved()).collect();
        ensure!(open.len() == 1, "the merged item is open for Q2");
        ensure!(wait_eligibility_matches(state, &v, "sess-merge")? == 1, "and notifiable");
        Ok(())
    })
}

/// P2 and Q10 (one epoch, both captured before the owner acts), the owner
/// resolving episode 0 when only `seen` had arrived, the other, then C5.
/// Steps: 0 session, 1 t1, 2 the seen positive, 3 owner Resolve, 4 the
/// other positive, 5 C5. A history where the owner saw P2 and one where it
/// saw Q10 differ: semantic equality must tell them apart.
fn wait_owner_coverage(name: &str, seen: u64) -> Builder {
    let mut b = Builder::new(name, "wait-events");
    let s = session(CLAUDE_LIKE, "sess-coverage");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(1).push();
    let shown = input_wait(&mut b, &s, Some("t1"), seen, "POSITIVE");
    let resolve = b.owner("cmd-resolve-episode", wait_target(&s, Some("t1"), seen), OwnerAction::Resolve { reason: "handled".into() }, &[shown]);
    let later = input_wait(&mut b, &s, Some("t1"), if seen == 2 { 10 } else { 2 }, "POSITIVE");
    b.before(resolve, later);
    let c5 = input_wait(&mut b, &s, Some("t1"), 5, "CLEARED");
    b.before(resolve, c5);
    b
}

/// The owner saw P2: after C5 clears it, Q10 (episode 1) is open.
pub fn wait_owner_covered_p() -> Scenario {
    wait_owner_coverage("wait-owner-covered-p", 2).build(|state| {
        let v = View::new(state);
        ensure!(decisions(state, "sess-coverage") == vec![("cmd-resolve-episode".into(), vec!["2".into()])], "covers P2");
        ensure!(wait_eligibility_matches(state, &v, "sess-coverage")? == 1, "Q10 is open");
        Ok(())
    })
}

/// The owner saw Q10: after C5, Q10 (episode 1) stays resolved.
pub fn wait_owner_covered_q() -> Scenario {
    wait_owner_coverage("wait-owner-covered-q", 10).build(|state| {
        let v = View::new(state);
        ensure!(decisions(state, "sess-coverage") == vec![("cmd-resolve-episode".into(), vec!["10".into()])], "covers Q10");
        ensure!(wait_eligibility_matches(state, &v, "sess-coverage")? == 0, "nothing open");
        Ok(())
    })
}

/// P2 and Q10 both arrive before the owner resolves: whatever their order,
/// the decision covers both, and after C5 nothing is open.
pub fn wait_owner_covers_both() -> Scenario {
    let mut b = Builder::new("wait-owner-covers-both", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-coverage");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(1).push();
    let p2 = input_wait(&mut b, &s, Some("t1"), 2, "POSITIVE");
    let q10 = input_wait(&mut b, &s, Some("t1"), 10, "POSITIVE");
    let resolve = b.owner("cmd-resolve-episode", wait_target(&s, Some("t1"), 2), OwnerAction::Resolve { reason: "handled".into() }, &[p2, q10]);
    let c5 = input_wait(&mut b, &s, Some("t1"), 5, "CLEARED");
    b.before(resolve, c5);
    b.build(|state| {
        let v = View::new(state);
        ensure!(decisions(state, "sess-coverage") == vec![("cmd-resolve-episode".into(), vec!["10".into(), "2".into()])], "covers P2 and Q10");
        ensure!(wait_eligibility_matches(state, &v, "sess-coverage")? == 0, "nothing open");
        Ok(())
    })
}

/// t1's wait resolved by the owner, t2's wait untouched, and a turnless
/// inventory wait acknowledged: each decision stays on its own scope.
pub fn wait_owner_scopes() -> Scenario {
    let mut b = Builder::new("wait-owner-scopes", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-owner-scopes");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(2).push();
    let p3 = input_wait(&mut b, &s, Some("t1"), 3, "POSITIVE");
    b.owner("cmd-resolve-t1", wait_target(&s, Some("t1"), 3), OwnerAction::Resolve { reason: "handled t1".into() }, &[p3]);
    b.obs(Some(&s), "turn.start").turn("t2").sequence(6).push();
    input_wait(&mut b, &s, Some("t2"), 7, "POSITIVE");
    let p8 = input_wait(&mut b, &s, None, 8, "POSITIVE");
    b.owner("cmd-ack-session", wait_target(&s, None, 8), OwnerAction::Acknowledge, &[p8]);
    b.build(|state| {
        let v = View::new(state);
        let t1 = v.turn("sess-owner-scopes", None, "t1").ok_or("t1")?.id.clone();
        let t2 = v.turn("sess-owner-scopes", None, "t2").ok_or("t2")?.id.clone();
        for scope in state.waits.values() {
            let commands: Vec<&str> = scope.owner_decisions.iter().map(|d| d.command_id.as_str()).collect();
            let expected: &[&str] = match scope.turn_id.as_deref() {
                Some(t) if t == t1 => &["cmd-resolve-t1"],
                Some(t) if t == t2 => &[],
                Some(_) => return Err("a wait on an unknown turn".into()),
                None => &["cmd-ack-session"],
            };
            ensure!(commands == expected, "decisions {commands:?} on the wait of turn {:?}", scope.turn_id);
        }
        for item in v.wait_items("sess-owner-scopes") {
            let owner = resolved_by(item, ResolutionKind::Owner);
            match item.turn_id.as_deref() {
                Some(t) if t == t1 => ensure!(owner && !item.acknowledged(), "t1 resolved by the owner"),
                Some(t) if t == t2 => ensure!(!owner && !item.acknowledged(), "t2 untouched"),
                _ => ensure!(!owner && item.acknowledged(), "the session wait acknowledged only"),
            }
        }
        ensure!(wait_eligibility_matches(state, &v, "sess-owner-scopes")? == 1, "t2's wait alone is notifiable");
        Ok(())
    })
}

/// Witness A's shape for Acknowledge (t1) and Snooze (t2): each covered P
/// alone, so the incomparable Q that joins its episode keeps the item
/// unacknowledged and unsnoozed.
pub fn wait_owner_partial_actions() -> Scenario {
    let mut b = Builder::new("wait-owner-partial-actions", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-partial-actions");
    b.obs(Some(&s), "session.start").sequence(1).push();
    for (turn, p, c, q, action) in [
        ("t1", 2, 3, 1, OwnerAction::Acknowledge),
        ("t2", 12, 13, 11, OwnerAction::Snooze { until_ms: 4_102_444_800_000 }),
    ] {
        b.obs(Some(&s), "turn.start").turn(turn).sequence(1).push();
        let positive = input_wait(&mut b, &s, Some(turn), p, "POSITIVE");
        let owner = b.owner(&format!("cmd-{turn}"), wait_target(&s, Some(turn), p), action, &[positive]);
        let clear = input_wait(&mut b, &s, Some(turn), c, "CLEARED");
        let other = other_epoch_wait(&mut b, &s, turn, q, "POSITIVE");
        b.before(owner, clear);
        b.before(owner, other);
    }
    b.build(|state| {
        let v = View::new(state);
        ensure!(
            decisions(state, "sess-partial-actions") == vec![("cmd-t1".into(), vec!["2".into()]), ("cmd-t2".into(), vec!["12".into()])],
            "each decision covers its P alone"
        );
        for item in v.wait_items("sess-partial-actions") {
            ensure!(!item.acknowledged() && item.snoozed_until_ms.is_none() && !item.resolved(), "Q keeps each item open");
        }
        ensure!(wait_eligibility_matches(state, &v, "sess-partial-actions")? == 2, "both notifiable");
        Ok(())
    })
}

/// P3 and U1 (a wait positive without a causal point) arrive before the
/// owner resolves; then U2, another such positive. Positives without a
/// causal point have no identity, so the decision records that it covered
/// one of them: U2, which the owner never saw, keeps the item open.
pub fn wait_owner_unordered_coverage() -> Scenario {
    let mut b = Builder::new("wait-owner-unordered-coverage", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-unordered-wait");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(2).push();
    let p3 = input_wait(&mut b, &s, Some("t1"), 3, "POSITIVE");
    let unordered = json!({ "category": "INPUT", "signal": "POSITIVE" });
    let u1 = b.obs(Some(&s), "wait").turn("t1").unordered().payload(unordered.clone()).push();
    let resolve = b.owner("cmd-resolve-unordered", wait_target(&s, Some("t1"), 3), OwnerAction::Resolve { reason: "handled".into() }, &[p3, u1]);
    let u2 = b.obs(Some(&s), "wait").turn("t1").unordered().payload(unordered).push();
    b.before(resolve, u2);
    b.build(|state| {
        let v = View::new(state);
        ensure!(decisions(state, "sess-unordered-wait") == vec![("cmd-resolve-unordered".into(), vec!["3".into()])], "covers P3");
        let counts: Vec<u32> = state.waits.values().flat_map(|w| &w.owner_decisions).map(|d| d.unordered).collect();
        ensure!(counts == [1], "and one positive without a causal point, got {counts:?}");
        ensure!(wait_eligibility_matches(state, &v, "sess-unordered-wait")? == 1, "U2 keeps the item notifiable");
        Ok(())
    })
}

// ------------------------------------------------------------------ follow-up

fn followup(name: &str, profile: &str, owner: bool) -> Builder {
    let mut b = Builder::new(name, "output-input");
    let native = format!("sess-{name}");
    let s = session(profile, &native);
    b.obs(Some(&s), "session.start").push();
    b.obs(Some(&s), "turn.start").turn("t1").push();
    b.obs(Some(&s), "notify.output").turn("t1").payload(json!({ "nativeKey": "t1-output" })).push();
    complete(&mut b, &s, "t1", "answer");
    let output_done = b.len() - 1;
    // A human follow-up after the output: accepted later.
    b.obs(Some(&s), "prompt.submit").prompt("p2").payload(json!({ "origin": "HUMAN_COMPOSER" })).push();
    b.obs(Some(&s), "prompt.accepted").prompt("p2").payload(accepted()).push();
    b.obs(Some(&s), "turn.start").turn("t2").prompt("p2").push();
    // Queued input entered while t2 runs, before t2's output.
    let p3 = b.obs(Some(&s), "prompt.submit").turn("t2").prompt("p3").payload(json!({ "origin": "HUMAN_COMPOSER" })).push();
    b.obs(Some(&s), "notify.output").turn("t2").push();
    complete(&mut b, &s, "t2", "answer");
    let p3_seq = b.step_sequence(p3).unwrap_or(0);
    b.obs(Some(&s), "prompt.accepted").prompt("p3").payload(accepted()).push();
    // An upstream-delayed submission: captured late, submitted before t2's output.
    b.wait_ms(500);
    b.obs(Some(&s), "prompt.submit").prompt("p4").payload(json!({
        "origin": "HUMAN_BRIDGE",
        "submission": {
            "sourceId": "synthetic.mod", "sourceEpoch": "mod-epoch-1", "orderDomain": "capture",
            "sequence": p3_seq.to_string(), "nativeKey": null, "nativePredecessorKeys": []
        }
    })).push();
    b.obs(Some(&s), "prompt.accepted").prompt("p4").payload(accepted()).push();
    // A scheduled prompt is never a human follow-up.
    b.obs(Some(&s), "prompt.submit").prompt("p5").payload(json!({ "origin": "SCHEDULED" })).push();
    b.obs(Some(&s), "prompt.accepted").prompt("p5").payload(accepted()).push();
    if owner {
        b.owner(
            "cmd-mark-handled-t1",
            Target::TurnOutput { session: s.clone(), agent: None, turn: "t1".into() },
            OwnerAction::Resolve { reason: "Mark handled".into() },
            &[output_done],
        );
    }
    b
}

pub fn followup_witnessed() -> Scenario {
    followup("followup-witnessed", WITNESSED, false).build(|state| {
        let v = View::new(state);
        let s = "sess-followup-witnessed";
        let t1 = v.turn(s, None, "t1").ok_or("t1")?;
        let t1_item = v.output_item(t1).ok_or("t1 item")?;
        ensure!(resolved_by(t1_item, ResolutionKind::HumanFollowup), "t1 output resolved by later accepted human input");
        let t2 = v.turn(s, None, "t2").ok_or("t2")?;
        let t2_item = v.output_item(t2).ok_or("t2 item")?;
        ensure!(!t2_item.resolved(), "input queued (p3) or submitted upstream (p4) before t2's output cannot resolve it; scheduled p5 never can");
        Ok(())
    })
}

pub fn followup_claude() -> Scenario {
    followup("followup-claude", CLAUDE_LIKE, true).build(|state| {
        let v = View::new(state);
        let s = "sess-followup-claude";
        let t1 = v.turn(s, None, "t1").ok_or("t1")?;
        let t1_item = v.output_item(t1).ok_or("t1 item")?;
        ensure!(!resolved_by(t1_item, ResolutionKind::HumanFollowup), "automaticHumanFollowupResolution is NOT_SUPPORTED (D-0005)");
        ensure!(resolved_by(t1_item, ResolutionKind::Owner), "explicit Mark handled resolves it");
        let t2 = v.turn(s, None, "t2").ok_or("t2")?;
        ensure!(!v.output_item(t2).ok_or("t2 item")?.resolved(), "t2 unresolved");
        ensure!(state.frontiers.len() == 1, "accepted-human provenance still recorded");
        Ok(())
    })
}

// ------------------------------------------------------------------ owner commands

pub fn owner_commands() -> Scenario {
    let mut b = Builder::new("owner-commands", "owner-commands");
    let s = session(CLAUDE_LIKE, "sess-owner");
    b.obs(Some(&s), "session.start").push();
    b.obs(Some(&s), "turn.start").turn("t1").push();
    let out1 = complete(&mut b, &s, "t1", "answer");
    b.obs(Some(&s), "turn.start").turn("t2").push();
    let out2 = complete(&mut b, &s, "t2", "error");
    let t1 = Target::TurnOutput { session: s.clone(), agent: None, turn: "t1".into() };
    let t2 = Target::TurnOutput { session: s.clone(), agent: None, turn: "t2".into() };
    b.owner("cmd-ack-1", t1.clone(), OwnerAction::Acknowledge, &[out1]);
    // A retry of the committed command: same ID and payload.
    b.owner("cmd-ack-1", t1.clone(), OwnerAction::Acknowledge, &[out1]);
    b.owner("cmd-snooze-2", t2.clone(), OwnerAction::Snooze { until_ms: 1_791_000_900_000 }, &[out2]);
    b.owner("cmd-resolve-2", t2.clone(), OwnerAction::Resolve { reason: "handled in terminal".into() }, &[out2]);
    // Another retry after the item's revision advanced.
    b.owner("cmd-ack-1", t1, OwnerAction::Acknowledge, &[out1]);
    b.owner("cmd-resolve-2", t2, OwnerAction::Resolve { reason: "handled in terminal".into() }, &[out2]);
    b.build(|state| {
        let v = View::new(state);
        let t1 = v.output_item(v.turn("sess-owner", None, "t1").ok_or("t1")?).ok_or("t1 item")?;
        ensure!(t1.acknowledged() && !t1.resolved(), "acknowledged is not resolved (INV-22)");
        ensure!(t1.acknowledgements.len() == 1, "the retried command applied once");
        let t2 = v.output_item(v.turn("sess-owner", None, "t2").ok_or("t2")?).ok_or("t2 item")?;
        ensure!(resolved_by(t2, ResolutionKind::Owner), "owner resolution with reason");
        ensure!(t2.snoozed_until_ms == Some(1_791_000_900_000), "snooze recorded");
        ensure!(state.commands.len() == 3, "three distinct commands, got {}", state.commands.len());
        Ok(())
    })
}

// ------------------------------------------------------------------ activations

pub fn resume() -> Scenario {
    let mut b = Builder::new("resume", "activation-changes");
    let s = session(CLAUDE_LIKE, "sess-resume");
    b.obs(Some(&s), "session.start").payload(json!({ "source": "startup" })).push();
    attach(&mut b, &s, "act-1", 5100, 1000, 16_777_230);
    bind(&mut b, &s, "act-1", "/dev/ttys010", 16_777_230, "5100:1000");
    b.obs(Some(&s), "turn.start").turn("t1").push();
    complete(&mut b, &s, "t1", "answer");
    b.obs(Some(&s), "execution.end").activation("act-1").payload(json!({ "reason": "prompt_input_exit" })).push();
    b.obs(Some(&s), "process.exit").provider(process(5100, 1000), CLAUDE_EXE, None).push();
    b.obs(Some(&s), "session.start").payload(json!({ "source": "resume" })).push();
    attach(&mut b, &s, "act-2", 5200, 2000, 16_777_231);
    bind(&mut b, &s, "act-2", "/dev/ttys011", 16_777_231, "5200:2000");
    b.obs(Some(&s), "turn.start").turn("t2").push();
    b.build(|state| {
        let v = View::new(state);
        ensure!(v.sessions_named("sess-resume") == 1, "resume keeps the persistent Session");
        let a1 = v.execution("sess-resume", "act-1").ok_or("act-1")?;
        let a2 = v.execution("sess-resume", "act-2").ok_or("act-2")?;
        ensure!(a1.presence == ExecutionPresence::Ended && a2.presence == ExecutionPresence::Live, "activation changed");
        ensure!(v.bindings(a1).iter().all(|b| !b.valid), "old route retired");
        ensure!(v.bindings(a2).iter().all(|b| b.valid), "new route proven");
        let session = v.session("sess-resume").ok_or("s")?;
        ensure!(session.execution_presence == ExecutionPresence::Live, "live again");
        ensure!(session.start_sources.contains("resume"), "resume source recorded");
        ensure!(turn_state(&v, "sess-resume", "t1")? == TurnState::Completed, "old turn kept");
        Ok(())
    })
}

pub fn pid_tty_reuse() -> Scenario {
    let mut b = Builder::new("pid-tty-reuse", "activation-changes");
    let s = session(CLAUDE_LIKE, "sess-reuse");
    let other = session(CLAUDE_LIKE, "sess-reuse-other");
    b.obs(Some(&s), "session.start").push();
    attach(&mut b, &s, "act-1", 700, 1000, 16_777_240);
    bind(&mut b, &s, "act-1", "/dev/ttys020", 16_777_240, "700:1000");
    b.obs(Some(&s), "process.exit").provider(process(700, 1000), CLAUDE_EXE, None).push();
    // PID 700 reused with a new kernel birth, on the same TTY path/device.
    b.obs(Some(&other), "session.start").push();
    attach(&mut b, &other, "act-x", 700, 2000, 16_777_240);
    bind(&mut b, &other, "act-x", "/dev/ttys020", 16_777_240, "700:2000");
    // A delayed re-proof of the old activation's surface arrives last.
    bind(&mut b, &s, "act-1", "/dev/ttys020", 16_777_240, "700:1000");
    b.obs(Some(&s), "execution.attach").activation("act-1").provider(process(700, 1000), CLAUDE_EXE, Some(16_777_240))
        .payload(json!({ "mode": "terminal_embedded", "presence": "LIVE", "device": 16_777_240 })).push();
    b.build(|state| {
        let v = View::new(state);
        ensure!(state.processes.len() == 2, "PID reuse is a new ProcessIncarnation");
        let old = v.execution("sess-reuse", "act-1").ok_or("act-1")?;
        ensure!(old.presence == ExecutionPresence::Ended, "old incarnation stays ended");
        ensure!(v.bindings(old).iter().all(|b| !b.valid), "reused PID/TTY never revives the old binding");
        let new = v.execution("sess-reuse-other", "act-x").ok_or("act-x")?;
        ensure!(v.bindings(new).iter().filter(|b| b.valid).count() == 1, "new binding valid");
        ensure!(state.surfaces.len() == 2, "TTY reuse is a new SourceSurface");
        ensure!(state.bindings.values().filter(|b| b.valid).count() == 1, "exactly one valid binding");
        Ok(())
    })
}

pub fn executable_replaced() -> Scenario {
    let mut b = Builder::new("executable-replaced", "activation-changes");
    let s = session(CLAUDE_LIKE, "sess-exec");
    b.obs(Some(&s), "session.start").push();
    attach(&mut b, &s, "act-1", 800, 1000, 16_777_250);
    bind(&mut b, &s, "act-1", "/dev/ttys030", 16_777_250, "800:1000");
    b.obs(Some(&s), "process.exec").provider(process(800, 1000), "/bin/zsh", None).push();
    b.build(|state| {
        let v = View::new(state);
        let execution = v.execution("sess-exec", "act-1").ok_or("act-1")?;
        ensure!(v.bindings(execution).iter().all(|b| !b.valid && b.invalidation_reason.as_deref() == Some("EXECUTABLE_REPLACED")), "exec in place invalidates the provider-process proof");
        Ok(())
    })
}

/// After a determinable replacement, an image with no causal point (as
/// reconciliation reports) cannot make the lost proof valid again.
pub fn replaced_then_unordered_image() -> Scenario {
    let mut b = Builder::new("replaced-then-unordered-image", "activation-changes");
    let s = session(CLAUDE_LIKE, "sess-exec-unordered");
    b.obs(Some(&s), "session.start").push();
    attach(&mut b, &s, "act-1", 820, 1000, 16_777_252);
    bind(&mut b, &s, "act-1", "/dev/ttys032", 16_777_252, "820:1000");
    b.obs(Some(&s), "process.exec").provider(process(820, 1000), "/bin/zsh", None).push();
    b.obs(Some(&s), "process.exec").provider(process(820, 1000), "/usr/bin/env", None).unordered().push();
    b.build(|state| {
        let v = View::new(state);
        let execution = v.execution("sess-exec-unordered", "act-1").ok_or("act-1")?;
        ensure!(v.bindings(execution).iter().all(|b| !b.valid && b.invalidation_reason.as_deref() == Some("EXECUTABLE_REPLACED")), "a lost proof stays lost");
        Ok(())
    })
}

/// The provider re-execs a new image in place; a fresh activation proven with
/// the new image requalifies, while the old proof stays invalid.
pub fn executable_requalified() -> Scenario {
    let mut b = Builder::new("executable-requalified", "activation-changes");
    let s = session(CLAUDE_LIKE, "sess-requalify");
    b.obs(Some(&s), "session.start").push();
    // A1 is proven with executable A; the process then execs B, so A1's
    // proof is lost for good, even when A is later observed again.
    attach(&mut b, &s, "act-1", 810, 1000, 16_777_251);
    bind(&mut b, &s, "act-1", "/dev/ttys031", 16_777_251, "810:1000");
    b.obs(Some(&s), "process.exec").provider(process(810, 1000), "/bin/zsh", None).push();
    b.obs(Some(&s), "process.exec").provider(process(810, 1000), CLAUDE_EXE, None).unordered().push();
    b.obs(Some(&s), "execution.end").activation("act-1").payload(json!({ "reason": "EXECUTABLE_REPLACED" })).push();
    // The same process returns to A and A2 is proven afresh with A.
    b.obs(Some(&s), "process.exec").provider(process(810, 1000), CLAUDE_EXE, None).push();
    attach(&mut b, &s, "act-2", 810, 1000, 16_777_251);
    bind(&mut b, &s, "act-2", "/dev/ttys031", 16_777_251, "810:1000");
    b.build(|state| {
        let v = View::new(state);
        let old = v.execution("sess-requalify", "act-1").ok_or("act-1")?;
        let new = v.execution("sess-requalify", "act-2").ok_or("act-2")?;
        ensure!(v.bindings(old).iter().all(|b| !b.valid), "old image's proof stays invalid");
        ensure!(!v.bindings(new).is_empty() && v.bindings(new).iter().all(|b| b.valid), "a fresh proof with A requalifies");
        Ok(())
    })
}

pub fn routing_ambiguity() -> Scenario {
    let mut b = Builder::new("routing-ambiguity", "activation-changes");
    let s = session(CLAUDE_LIKE, "sess-ambiguous");
    b.obs(Some(&s), "session.start").push();
    attach(&mut b, &s, "act-1", 900, 1000, 16_777_260);
    bind(&mut b, &s, "act-1", "/dev/ttys040", 16_777_260, "900:1000");
    attach(&mut b, &s, "act-2", 901, 1001, 16_777_261);
    bind(&mut b, &s, "act-2", "/dev/ttys041", 16_777_261, "901:1001");
    b.build(|state| {
        let v = View::new(state);
        ensure!(v.sessions_named("sess-ambiguous") == 1, "one worker");
        let valid = state.bindings.values().filter(|b| b.valid).count();
        ensure!(valid == 2, "two live attachments both kept (a chooser, never newest)");
        Ok(())
    })
}

pub fn execution_end_retained() -> Scenario {
    let mut b = Builder::new("execution-end-retained", "activation-changes");
    let s = session(CLAUDE_LIKE, "sess-ended");
    b.obs(Some(&s), "session.start").push();
    attach(&mut b, &s, "act-1", 1000, 1000, 16_777_270);
    b.obs(Some(&s), "turn.start").turn("t1").push();
    complete(&mut b, &s, "t1", "answer");
    b.obs(Some(&s), "execution.end").activation("act-1").payload(json!({ "reason": "logout" })).push();
    b.build(|state| {
        let v = View::new(state);
        let session = v.session("sess-ended").ok_or("session retained after completion and end")?;
        ensure!(session.execution_presence == ExecutionPresence::Ended, "ended");
        let item = v.output_item(v.turn("sess-ended", None, "t1").ok_or("t1")?).ok_or("item")?;
        ensure!(!item.resolved(), "unresolved owner action survives execution end");
        Ok(())
    })
}

/// A→B→A in one process with a delayed completion and a delayed old end.
pub fn a_b_a_delayed() -> Scenario {
    let mut b = Builder::new("a-b-a-delayed", "delayed-callbacks");
    let a = session(CLAUDE_LIKE, "sess-A");
    let bb = session(CLAUDE_LIKE, "sess-B");
    b.obs(Some(&a), "session.start").push();
    attach(&mut b, &a, "act-A1", 1100, 1000, 16_777_280);
    b.obs(Some(&a), "turn.start").turn("tA1").push();
    b.obs(Some(&a), "execution.end").activation("act-A1").payload(json!({ "reason": "clear" })).push();
    b.obs(Some(&bb), "session.start").payload(json!({ "source": "clear" })).push();
    attach(&mut b, &bb, "act-B1", 1100, 1000, 16_777_280);
    b.obs(Some(&bb), "execution.end").activation("act-B1").payload(json!({ "reason": "resume" })).push();
    b.obs(Some(&a), "session.start").payload(json!({ "source": "resume" })).push();
    attach(&mut b, &a, "act-A2", 1100, 1000, 16_777_280);
    b.obs(Some(&a), "turn.start").turn("tA2").push();
    // Delayed: tA1's completion and act-A1's end arrive after A2 began.
    complete(&mut b, &a, "tA1", "answer");
    b.obs(Some(&a), "execution.end").activation("act-A1").payload(json!({ "reason": "clear" })).push();
    b.build(|state| {
        let v = View::new(state);
        ensure!(v.execution("sess-A", "act-A1").ok_or("A1")?.presence == ExecutionPresence::Ended, "A1 ended");
        ensure!(v.execution("sess-A", "act-A2").ok_or("A2")?.presence == ExecutionPresence::Live, "an old end closes only its own activation");
        ensure!(v.execution("sess-B", "act-B1").ok_or("B1")?.presence == ExecutionPresence::Ended, "B1 ended");
        ensure!(turn_state(&v, "sess-A", "tA1")? == TurnState::Completed, "delayed completion settles its own turn");
        ensure!(turn_state(&v, "sess-A", "tA2")? == TurnState::Working, "and not the current one");
        Ok(())
    })
}

// ------------------------------------------------------------------ snapshots and coverage

pub fn stale_snapshot() -> Scenario {
    let mut b = Builder::new("stale-snapshot", "snapshot-live");
    let s = session(CLAUDE_LIKE, "sess-snap");
    let inventory = |b: &mut Builder, present: bool, status: &str, start: i64| {
        b.obs(Some(&s), "inventory.row")
            .source("synthetic.inventory", "inv-epoch-1")
            .payload(json!({
                "present": present,
                "row": { "kind": "interactive", "status": status, "waitingFor": null, "displayName": null },
                "interval": { "startMs": start, "endMs": start + 40 },
            }))
            .push()
    };
    inventory(&mut b, true, "busy", 1_000);
    b.obs(Some(&s), "turn.start").turn("t1").push();
    inventory(&mut b, true, "idle", 2_000);
    complete(&mut b, &s, "t1", "answer");
    inventory(&mut b, false, "idle", 3_000);
    b.build(|state| {
        let v = View::new(state);
        let session = v.session("sess-snap").ok_or("s")?;
        ensure!(session.observation == ObservationState::Stale, "the newest snapshot (absent) wins; an older one never overwrites it");
        ensure!(session.record_state == threadspace_contracts::canonical::fact::SessionRecordState::Known, "absence is not deletion");
        ensure!(turn_state(&v, "sess-snap", "t1")? == TurnState::Completed, "a snapshot never overwrites a concrete outcome");
        Ok(())
    })
}

pub fn dropped_observation() -> Scenario {
    let mut b = Builder::new("dropped-observation", "snapshot-live");
    let s = session(CLAUDE_LIKE, "sess-gap");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(2).push();
    // Sequence 3 (a step) was dropped before durable acceptance.
    b.obs(Some(&s), "turn.step").turn("t1").sequence(4).push();
    b.obs(Some(&s), "Stop").turn("t1").sequence(5).push();
    b.build(|state| {
        let v = View::new(state);
        let coverage = state.coverage.values().next().ok_or("coverage")?;
        ensure!(coverage.gaps.len() == 1 && coverage.gaps[0].first == "3", "the source gap is recorded");
        ensure!(turn_state(&v, "sess-gap", "t1")? == TurnState::Working, "a gap invents no completion");
        ensure!(v.session("sess-gap").ok_or("s")?.execution_presence == ExecutionPresence::Unknown, "unknown stays unknown");
        Ok(())
    })
}

// ------------------------------------------------------------------ robustness

pub fn unknown_and_malformed() -> Scenario {
    let mut b = Builder::new("unknown-and-malformed", "robustness");
    let s = session(CLAUDE_LIKE, "sess-robust");
    b.obs(Some(&s), "session.start").push();
    b.obs(Some(&s), "turn.teleport").turn("t1").push();
    b.obs(Some(&s), "turn.complete").turn("t1").payload(json!({ "reason": "exploded" })).push();
    b.obs(Some(&s), "execution.attach").activation("act-1").payload(json!({ "presence": "LIVE" })).push();
    b.obs(Some(&s), "wait").payload(json!({ "category": "PONDERING", "signal": "POSITIVE" })).push();
    b.obs(None, "turn.start").turn("t-orphan").push();
    b.build(|state| {
        ensure!(state.turns.is_empty(), "no unknown discriminator or orphan drove a turn");
        ensure!(state.executions.is_empty(), "malformed attach drove nothing");
        ensure!(state.waits.is_empty(), "unknown wait category drove nothing");
        ensure!(state.sessions.len() == 1, "only the valid session");
        Ok(())
    })
}

pub fn sensitive_payload() -> Scenario {
    let mut b = Builder::new("sensitive-payload", "robustness");
    let s = session(CLAUDE_LIKE, "sess-sensitive");
    b.obs(Some(&s), "session.start").push();
    b.obs(Some(&s), "prompt.submit").prompt("p1").payload(json!({
        "origin": "HUMAN_COMPOSER",
        "prompt": crate::SECRET_PROMPT,
        "toolInput": { "command": crate::SECRET_TOOL },
    })).push();
    b.obs(Some(&s), "tool.call").turn("t1").occurrence("tool-1").payload(json!({
        "phase": "finished", "toolCategory": "bash", "result": "SUCCESS",
        "toolResponse": crate::SECRET_TOOL,
    })).push();
    b.build(|state| {
        let text = serde_json::to_string(state).map_err(|e| e.to_string())?;
        ensure!(!text.contains(crate::SECRET_PROMPT) && !text.contains(crate::SECRET_TOOL), "no body reaches canonical state");
        ensure!(state.inputs.len() == 1 && state.activities.len() == 1, "metadata still observed");
        Ok(())
    })
}

// ---------------------------------------------------------------- M2 (D-0010)
//
// Each field below took the latest arrival before reducer 3. Every scenario
// holds reports that only their causal points order, and reports nothing
// orders; the campaign reorders and duplicates them, so any dependence on
// arrival order shows as a diverging semantic hash.

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

/// Attach mode and presence (D-0007 §10 (1)): an ordered pair settles on the
/// later report; reports nothing orders settle on the least live presence and
/// are flagged; the device is the newest one any report names.
pub fn attachment_evidence() -> Scenario {
    let mut b = Builder::new("attachment-evidence", "evidence-sets");
    let s = session(CLAUDE_LIKE, "sess-attachment-evidence");
    b.obs(Some(&s), "session.start").push();
    attach_as(&mut b, &s, "act-ordered", "LIVE", 7, None);
    attach_as(&mut b, &s, "act-ordered", "DETACHED", 8, None);
    attach_as(&mut b, &s, "act-ordered", "LIVE", 9, None);
    attach_as(&mut b, &s, "act-unordered", "LIVE", 7, None);
    attach_as(&mut b, &s, "act-unordered", "DETACHED", 7, Some(("synthetic.other", "other-1")));
    b.build(|state| {
        let v = View::new(state);
        let s = "sess-attachment-evidence";
        let ordered = v.execution(s, "act-ordered").ok_or("ordered execution")?;
        ensure!(ordered.presence == ExecutionPresence::Live && !ordered.attachment_conflict, "the causally latest report decides: LIVE");
        ensure!(ordered.controlling_device == Some(9), "the newest device: {:?}", ordered.controlling_device);
        let unordered = v.execution(s, "act-unordered").ok_or("unordered execution")?;
        ensure!(unordered.attachment_conflict, "unordered disagreement is flagged");
        ensure!(unordered.presence == ExecutionPresence::Detached, "unordered disagreement settles least live: {:?}", unordered.presence);
        ensure!(unordered.controlling_device == Some(7), "agreeing devices stand");
        Ok(())
    })
}

/// The human-follow-up frontier (D-0007 §10 (2)): a rejection withdraws the
/// frontier an acceptance advanced, in any order; disagreeing origins never
/// qualify; an accepted human input alone still resolves (the control).
pub fn followup_frontier_evidence() -> Scenario {
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
    b.build(|state| {
        let v = View::new(state);
        let item = |s: &str| -> Result<bool, String> {
            let t1 = v.turn(s, None, "t1").ok_or(format!("{s} t1"))?;
            Ok(resolved_by(v.output_item(t1).ok_or(format!("{s} item"))?, ResolutionKind::HumanFollowup))
        };
        ensure!(!item("sess-frontier-rejected")?, "a rejected input never resolves the earlier output");
        ensure!(!item("sess-frontier-origin")?, "an input with disagreeing origins never qualifies");
        ensure!(item("sess-frontier-control")?, "an accepted human input resolves the earlier output");
        let frontiers = |s: &str| {
            let id = v.session(s).map(|r| r.id.clone()).unwrap_or_default();
            state.frontiers.values().filter(|f| f.session_id == id).count()
        };
        ensure!(frontiers("sess-frontier-rejected") == 0, "no frontier survives the rejection");
        ensure!(frontiers("sess-frontier-control") == 1, "the control's frontier stands");
        Ok(())
    })
}

/// Observer link state (D-0007 §10 (3)): an ordered pair settles on the later
/// report; reports nothing orders settle on the most degraded and are flagged.
pub fn observer_link_evidence() -> Scenario {
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
    b.build(|state| {
        let v = View::new(state);
        let ordered = v.session("sess-link-ordered").ok_or("ordered session")?;
        ensure!(ordered.observation == ObservationState::Current && !ordered.link_conflict, "the causally latest report decides: {:?}", ordered.observation);
        ensure!(ordered.observer_tier == Some(ObserverTier::Native), "qualified engine-stamped reports are native");
        let unordered = v.session("sess-link-unordered").ok_or("unordered session")?;
        ensure!(unordered.link_conflict, "unordered disagreement is flagged");
        ensure!(unordered.observation == ObservationState::Disconnected, "unordered disagreement settles most degraded: {:?}", unordered.observation);
        Ok(())
    })
}

/// Host-read-tier outcomes (D-0005 reload rule, D-0010): an outcome whose
/// session attribution is a host read applies only once kernel/inventory
/// evidence shows its provider process running that Session, whichever
/// arrives first; one from another process never applies. A qualified
/// host-read link with a corroborated process is restored; without that
/// proof, or from an unqualified profile, it stays lower tier.
pub fn host_read_outcome() -> Scenario {
    let mut b = Builder::new("host-read-outcome", "evidence-sets");
    let corroborated = session(CLAUDE_LIKE, "sess-host-read-corroborated");
    b.obs(Some(&corroborated), "session.start").push();
    b.obs(Some(&corroborated), "execution.attach")
        .activation("act-p")
        .provider(process(81, 401), CLAUDE_EXE, Some(11))
        .payload(json!({ "mode": "terminal_embedded", "presence": "LIVE", "device": 11 }))
        .push();
    b.obs(Some(&corroborated), "turn.start").turn("t1").push();
    b.obs(Some(&corroborated), "turn.complete")
        .turn("t1")
        .provider(process(81, 401), CLAUDE_EXE, None)
        .payload(json!({ "reason": "answer", "tier": "HOST_READ" }))
        .source("synthetic.observer", "reloaded-1")
        .push();
    b.obs(Some(&corroborated), "observer.link")
        .provider(process(81, 401), CLAUDE_EXE, None)
        .payload(json!({ "link": "CURRENT", "tier": "HOST_READ" }))
        .source("synthetic.observer", "reloaded-1")
        .push();
    let foreign = session(CLAUDE_LIKE, "sess-host-read-foreign");
    b.obs(Some(&foreign), "session.start").push();
    b.obs(Some(&foreign), "execution.attach")
        .activation("act-q")
        .provider(process(82, 402), CLAUDE_EXE, Some(12))
        .payload(json!({ "mode": "terminal_embedded", "presence": "LIVE", "device": 12 }))
        .push();
    b.obs(Some(&foreign), "turn.start").turn("t1").push();
    b.obs(Some(&foreign), "turn.complete")
        .turn("t1")
        .provider(process(83, 403), CLAUDE_EXE, None)
        .payload(json!({ "reason": "answer", "tier": "HOST_READ" }))
        .source("synthetic.observer", "reloaded-2")
        .push();
    b.obs(Some(&foreign), "observer.link")
        .provider(process(83, 403), CLAUDE_EXE, None)
        .payload(json!({ "link": "CURRENT", "tier": "HOST_READ" }))
        .source("synthetic.observer", "reloaded-2")
        .push();
    let unqualified = session(CLAUDE_LIKE, "sess-host-read-unqualified");
    b.obs(Some(&unqualified), "session.start").push();
    b.obs(Some(&unqualified), "observer.link")
        .payload(json!({ "link": "CURRENT", "qualified": false, "version": "2.1.292" }))
        .push();
    b.build(|state| {
        let v = View::new(state);
        let turn = |s: &str| v.turn(s, None, "t1").map(|t| (t.state.clone(), t.pending_outcomes.len()));
        ensure!(
            turn("sess-host-read-corroborated") == Some((TurnState::Completed, 1)),
            "a corroborated host-read outcome applies: {:?}",
            turn("sess-host-read-corroborated")
        );
        ensure!(
            turn("sess-host-read-foreign") == Some((TurnState::Working, 1)),
            "an outcome from another process is retained, never applied: {:?}",
            turn("sess-host-read-foreign")
        );
        let tier = |s: &str| v.session(s).and_then(|r| r.observer_tier);
        ensure!(tier("sess-host-read-corroborated") == Some(ObserverTier::Restored), "corroborated host read is restored");
        ensure!(tier("sess-host-read-foreign") == Some(ObserverTier::LowerTier), "uncorroborated host read stays lower tier");
        ensure!(tier("sess-host-read-unqualified") == Some(ObserverTier::LowerTier), "an unqualified profile stays lower tier");
        Ok(())
    })
}

// ------------------------------------------------------------------ inventory waits

/// An inventory wait as a discovery pass reports it (SPEC §11.3): the
/// session's, with no turn or actor, ordered by the pass sequence of one
/// companion incarnation.
fn inventory_wait(b: &mut Builder, s: &NativeSessionRef, pass: u64, signal: &str) -> usize {
    let mut payload = json!({ "category": "INPUT", "signal": signal, "orderDomain": "inventory" });
    if signal == "POSITIVE" {
        payload["subtype"] = json!("input needed");
    }
    b.obs(Some(s), "wait").source("synthetic.inventory", "inv-epoch-1").sequence(pass).payload(payload).push()
}

/// The parent turn answered while a child agent's turn still runs.
fn parent_completed(name: &str, native: &str) -> (Builder, NativeSessionRef) {
    let mut b = Builder::new(name, "wait-events");
    let s = session(CLAUDE_LIKE, native);
    b.obs(Some(&s), "session.start").push();
    b.obs(Some(&s), "turn.start").turn("t1").push();
    b.obs(Some(&s), "agent.spawn").agent("a1").payload(json!({ "agentType": "general" })).push();
    b.obs(Some(&s), "turn.start").agent("a1").turn("a1-t1").push();
    complete(&mut b, &s, "t1", "answer");
    (b, s)
}

/// An inventory wait never reopens the completed parent turn or sets a
/// child turn WAITING; its item is the session's.
fn session_wait_only(state: &CanonicalState, native: &str) -> Result<(), String> {
    let v = View::new(state);
    ensure!(turn_state(&v, native, "t1")? == TurnState::Completed, "the parent turn stays COMPLETED");
    let child = v.turn(native, Some("a1"), "a1-t1").ok_or("child turn")?;
    ensure!(child.state == TurnState::Working, "the child turn is not WAITING");
    ensure!(v.session(native).ok_or("session")?.turn_state != TurnState::Waiting, "no turn is WAITING");
    let items = v.wait_items(native);
    ensure!(items.len() == 1, "one wait item, got {}", items.len());
    ensure!(items[0].turn_id.is_none() && items[0].actor_id.is_none(), "a session-scoped item");
    ensure!(items[0].category == AttentionCategory::InputRequired, "an input wait");
    Ok(())
}

pub fn parent_completed_child_waiting() -> Scenario {
    let (mut b, s) = parent_completed("parent-completed-child-waiting", "sess-parent-done");
    inventory_wait(&mut b, &s, 1, "POSITIVE");
    b.build(|state| {
        session_wait_only(state, "sess-parent-done")?;
        let v = View::new(state);
        ensure!(wait_eligibility_matches(state, &v, "sess-parent-done")? == 1, "the session's input wait is open and notifiable");
        Ok(())
    })
}

pub fn parent_completed_child_wait_cleared() -> Scenario {
    let (mut b, s) = parent_completed("parent-completed-child-wait-cleared", "sess-parent-cleared");
    inventory_wait(&mut b, &s, 1, "POSITIVE");
    inventory_wait(&mut b, &s, 2, "CLEARED");
    b.build(|state| {
        session_wait_only(state, "sess-parent-cleared")?;
        let v = View::new(state);
        ensure!(resolved_by(v.wait_items("sess-parent-cleared")[0], ResolutionKind::WaitEnded), "the later clear resolves it natively");
        ensure!(wait_eligibility_matches(state, &v, "sess-parent-cleared")? == 0, "nothing stays notifiable");
        Ok(())
    })
}

/// The clear arrives before the positive it ends: their pass points, not
/// arrival, order them.
pub fn parent_completed_child_wait_reordered() -> Scenario {
    let (mut b, s) = parent_completed("parent-completed-child-wait-reordered", "sess-parent-reordered");
    inventory_wait(&mut b, &s, 2, "CLEARED");
    inventory_wait(&mut b, &s, 1, "POSITIVE");
    b.build(|state| {
        session_wait_only(state, "sess-parent-reordered")?;
        let v = View::new(state);
        ensure!(resolved_by(v.wait_items("sess-parent-reordered")[0], ResolutionKind::WaitEnded), "the earlier-arriving clear still ends it");
        ensure!(wait_eligibility_matches(state, &v, "sess-parent-reordered")? == 0, "no live banner for a historical wait");
        Ok(())
    })
}

/// Every catalogued scenario.
pub fn catalog() -> Vec<Scenario> {
    vec![
        normal_session(),
        parallel_tools(),
        outcomes(),
        latest_turn_outcome(),
        duplicate_deliveries(),
        child_actors(),
        shared_native_turn_id(),
        waiting(),
        delayed_positive_wait(),
        delayed_clear_wait(),
        wait_reappears(),
        wait_uncertain_reopens(),
        wait_owner_resolution_kept(),
        wait_new_after_resolution(),
        wait_turn_ownership(),
        wait_session_scoped(),
        wait_generations(),
        followup_witnessed(),
        followup_claude(),
        owner_commands(),
        resume(),
        pid_tty_reuse(),
        executable_replaced(),
        replaced_then_unordered_image(),
        executable_requalified(),
        routing_ambiguity(),
        execution_end_retained(),
        a_b_a_delayed(),
        stale_snapshot(),
        dropped_observation(),
        unknown_and_malformed(),
        sensitive_payload(),
        // Appended, so earlier scenarios keep their catalog index and so
        // their permutation seeds.
        wait_owner_partial_coverage(),
        wait_owner_merge_keeps_coverage(),
        wait_owner_covered_p(),
        wait_owner_covered_q(),
        wait_owner_covers_both(),
        wait_owner_scopes(),
        wait_owner_partial_actions(),
        wait_owner_unordered_coverage(),
        // M2: the reducer-3 evidence sets (D-0010).
        attachment_evidence(),
        followup_frontier_evidence(),
        observer_link_evidence(),
        host_read_outcome(),
        // M2: session-scoped inventory waits.
        parent_completed_child_waiting(),
        parent_completed_child_wait_cleared(),
        parent_completed_child_wait_reordered(),
    ]
}
