//! Scenario execution. `Admit` is the admission boundary a runner offers;
//! the pure runner implements it in memory with the same normalization,
//! resolution and reduction the SQLite journal uses, and the invariant
//! monitor checks every intermediate state, not only the final one.

use std::collections::{BTreeMap, BTreeSet};

use threadspace_contracts::canonical::capture::RecordStatus;
use threadspace_contracts::canonical::command::{OwnerAction, OwnerCommand};
use threadspace_contracts::canonical::envelope::ObservationEnvelope;
use threadspace_contracts::canonical::fact::{Delivery, JournalEntry};
use threadspace_contracts::canonical::records::{AttentionScope, CanonicalState, ResolutionKind};
use threadspace_contracts::projection::{ExecutionPresence, TurnState};
use threadspace_contracts::canonical::causal::CausalOrder;
use threadspace_contracts::canonical::fact::WaitCategory;
use threadspace_state_engine::causal::compare;
use threadspace_state_engine::wait;
use threadspace_state_engine::command;
use threadspace_state_engine::engine::Engine;
use threadspace_state_engine::ids::{Allocator, SeededAllocator};
use threadspace_state_engine::keys;
use threadspace_state_engine::resolve::{IdentityIndex, Resolver};
use threadspace_state_engine::synthetic::{self, NormalizeError};

use crate::builder::{OwnerStep, Scenario, Step, Target};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub status: RecordStatus,
    pub cursor: Option<i64>,
    pub reason: Option<String>,
}

pub trait Admit {
    fn observe(&mut self, envelope: &ObservationEnvelope) -> Receipt;
    fn owner(&mut self, command: &OwnerCommand, at_ms: i64) -> Receipt;
    fn state(&self) -> &CanonicalState;
}

/// The attention item an owner step targets, as the owner's view finds it.
pub fn target_attention(state: &CanonicalState, target: &Target) -> Option<String> {
    let find_session = |session: &threadspace_contracts::canonical::keys::NativeSessionRef| {
        state.sessions.values().find(|s| {
            s.native_session_id == session.native_session_id
                && state.namespaces.get(&s.namespace_id).is_some_and(|n| {
                    n.provider == session.provider && n.profile_ref == session.profile_ref
                })
        })
    };
    match target {
        Target::TurnOutput {
            session,
            agent,
            turn,
        } => {
            let session = find_session(session)?;
            let turn = state.turns.values().find(|t| {
                t.session_id == session.id
                    && t.native_turn_id.as_deref() == Some(turn.as_str())
                    && state.actors.get(&t.actor_id).is_some_and(|a| match (&a.native, agent) {
                        (threadspace_contracts::canonical::keys::NativeActorRef::Principal, None) => true,
                        (
                            threadspace_contracts::canonical::keys::NativeActorRef::Agent { native_agent_id },
                            Some(agent),
                        ) => native_agent_id == agent,
                        _ => false,
                    })
            })?;
            state
                .attention_by_scope
                .get(&keys::attention_scope(&session.id, "TURN_OUTPUT", &turn.id))
                .cloned()
        }
        Target::Request { session, request } => {
            let session = find_session(session)?;
            state
                .attention_by_scope
                .get(&keys::attention_scope(&session.id, "EXACT_REQUEST", request))
                .cloned()
        }
        Target::Wait {
            session,
            turn,
            category,
            witness,
        } => {
            let session = find_session(session)?;
            let turn_id = match turn {
                Some(turn) => Some(
                    state
                        .turns
                        .values()
                        .find(|t| t.session_id == session.id && t.native_turn_id.as_deref() == Some(turn.as_str()))?
                        .id
                        .clone(),
                ),
                None => None,
            };
            state.waits.values().find_map(|scope| {
                if scope.session_id != session.id || scope.turn_id != turn_id || wait_kind(scope.category) != category {
                    return None;
                }
                let point = scope
                    .positives
                    .iter()
                    .find(|p| p.sequence.as_deref() == Some(witness.to_string().as_str()))?;
                let index = scope
                    .clears
                    .iter()
                    .filter(|clear| compare(clear, point) == CausalOrder::Before)
                    .count() as u32;
                scope.episodes.iter().find(|e| e.index == index)?.attention_id.clone()
            })
        }
    }
}

fn wait_kind(category: WaitCategory) -> &'static str {
    match category {
        WaitCategory::Approval => "APPROVAL",
        WaitCategory::Input => "INPUT",
        WaitCategory::JobBlocked => "JOB_BLOCKED",
    }
}

fn action_name(action: &OwnerAction) -> &'static str {
    match action {
        OwnerAction::Acknowledge => "Acknowledge",
        OwnerAction::Resolve { .. } => "Resolve",
        OwnerAction::Snooze { .. } => "Snooze",
    }
}

/// In-memory admission: dedup by observation UUID, pure normalization,
/// identity resolution with a seeded allocator, pure reduction.
pub struct PureRunner {
    pub engine: Engine,
    pub index: IdentityIndex,
    allocator: SeededAllocator,
    pub endpoint: String,
    cursor: i64,
    seen: BTreeMap<String, String>,
    commands: BTreeMap<String, (String, Receipt)>,
    pub entries: Vec<JournalEntry>,
    pub unresolved: u64,
    pub unsupported: u64,
    pub delivery: Delivery,
}

impl PureRunner {
    pub fn new(seed: u64) -> Self {
        let mut allocator = SeededAllocator::new(seed);
        let endpoint = allocator.allocate();
        Self {
            engine: Engine::empty(),
            index: IdentityIndex::default(),
            allocator,
            endpoint,
            cursor: 0,
            seen: BTreeMap::new(),
            commands: BTreeMap::new(),
            entries: Vec::new(),
            unresolved: 0,
            unsupported: 0,
            delivery: Delivery::Live,
        }
    }
}

impl Admit for PureRunner {
    fn observe(&mut self, envelope: &ObservationEnvelope) -> Receipt {
        let content = serde_json::to_string(envelope).unwrap_or_default();
        if let Some(previous) = self.seen.get(&envelope.observation_id) {
            return if *previous == content {
                Receipt {
                    status: RecordStatus::AlreadyCommitted,
                    cursor: None,
                    reason: None,
                }
            } else {
                Receipt {
                    status: RecordStatus::NotAccepted,
                    cursor: None,
                    reason: Some("OBSERVATION_ID_CONFLICT".into()),
                }
            };
        }
        let drafts = match synthetic::normalize(envelope) {
            Ok(drafts) => drafts,
            Err(NormalizeError::Unsupported(_) | NormalizeError::Malformed(_)) => {
                self.unsupported += 1;
                Vec::new()
            }
        };
        let mut resolver = Resolver::new(&self.index, &mut self.allocator, &self.endpoint);
        let (facts, unresolved) =
            resolver.resolve(&self.engine.state, &envelope.observation_id, &drafts);
        let assignments = resolver.into_assignments();
        self.unresolved += unresolved.len() as u64;
        self.cursor += 1;
        let entry = JournalEntry {
            cursor: self.cursor,
            payload_version: threadspace_contracts::canonical::JOURNAL_PAYLOAD_VERSION,
            endpoint_id: self.endpoint.clone(),
            observation_id: envelope.observation_id.clone(),
            source_id: envelope.source_id.clone(),
            source_epoch: envelope.source_epoch.clone(),
            source_sequence: envelope.source_sequence.clone(),
            sequence_meaning: envelope.sequence_meaning,
            captured_wall_ms: envelope.captured_at.wall_time_ms,
            delivery: self.delivery,
            facts,
        };
        self.engine.apply(&entry);
        self.index.apply(&assignments);
        self.entries.push(entry);
        self.seen.insert(envelope.observation_id.clone(), content);
        Receipt {
            status: RecordStatus::Committed,
            cursor: Some(self.cursor),
            reason: None,
        }
    }

    fn owner(&mut self, owner: &OwnerCommand, at_ms: i64) -> Receipt {
        let print = command::fingerprint(owner);
        if let Some((recorded, receipt)) = self.commands.get(&owner.command_id) {
            return if *recorded == print {
                Receipt {
                    status: RecordStatus::AlreadyCommitted,
                    ..receipt.clone()
                }
            } else {
                Receipt {
                    status: RecordStatus::NotAccepted,
                    cursor: None,
                    reason: Some("COMMAND_ID_CONFLICT".into()),
                }
            };
        }
        let Some(item) = self.engine.state.attention.get(&owner.attention_id) else {
            return Receipt {
                status: RecordStatus::NotAccepted,
                cursor: None,
                reason: Some("UNKNOWN_ATTENTION".into()),
            };
        };
        let session_id = item.session_id.clone();
        self.cursor += 1;
        let observation_id = self.allocator.allocate();
        let fact = command::fact(
            self.allocator.allocate(),
            &observation_id,
            &session_id,
            owner,
            at_ms,
        );
        let entry = JournalEntry {
            cursor: self.cursor,
            payload_version: threadspace_contracts::canonical::JOURNAL_PAYLOAD_VERSION,
            endpoint_id: self.endpoint.clone(),
            observation_id,
            source_id: command::OWNER_SOURCE.into(),
            source_epoch: "owner".into(),
            source_sequence: None,
            sequence_meaning: None,
            captured_wall_ms: at_ms,
            delivery: self.delivery,
            facts: vec![fact],
        };
        self.engine.apply(&entry);
        self.entries.push(entry);
        let receipt = Receipt {
            status: RecordStatus::Committed,
            cursor: Some(self.cursor),
            reason: None,
        };
        self.commands
            .insert(owner.command_id.clone(), (print, receipt.clone()));
        receipt
    }

    fn state(&self) -> &CanonicalState {
        &self.engine.state
    }
}

fn terminal(state: &TurnState) -> bool {
    matches!(
        state,
        TurnState::Completed | TurnState::Interrupted | TurnState::Failed | TurnState::Refused
    )
}

/// Monotonic guarantees checked after every delivered step.
#[derive(Debug, Default)]
pub struct Monitor {
    terminal_turns: BTreeSet<String>,
    lost_bindings: BTreeSet<String>,
    ended_executions: BTreeSet<String>,
    owner_resolved: BTreeSet<String>,
    acknowledged: BTreeSet<String>,
    wait_decisions: BTreeSet<(String, String)>,
    pub violations: Vec<String>,
}

impl Monitor {
    pub fn after(&mut self, step: &Step, before: &CanonicalState, state: &CanonicalState) {
        for turn in state.turns.values() {
            if self.terminal_turns.contains(&turn.id) && !terminal(&turn.state) {
                self.violations
                    .push(format!("turn {:?} regressed to {:?}", turn.native_turn_id, turn.state));
            }
            if terminal(&turn.state) {
                self.terminal_turns.insert(turn.id.clone());
            }
        }
        for binding in state.bindings.values() {
            if self.lost_bindings.contains(&binding.id) && binding.valid {
                self.violations.push(format!("binding {} revived", binding.id));
            }
            if binding
                .invalidation_reason
                .as_deref()
                .is_some_and(|reason| reason != "UNPROVEN")
            {
                self.lost_bindings.insert(binding.id.clone());
            }
        }
        let stop = matches!(step, Step::Observe(e) if e.native_event == "Stop")
            || matches!(step, Step::Observe(e) if e.native_event == "turn.complete");
        for execution in state.executions.values() {
            let was_ended = before
                .executions
                .get(&execution.id)
                .is_some_and(|e| e.presence == ExecutionPresence::Ended);
            if self.ended_executions.contains(&execution.id)
                && execution.presence != ExecutionPresence::Ended
            {
                self.violations.push(format!("execution {} left ENDED", execution.id));
            }
            if stop && !was_ended && execution.presence == ExecutionPresence::Ended {
                self.violations
                    .push(format!("{} ended execution {}", step.label(), execution.id));
            }
            if execution.presence == ExecutionPresence::Ended {
                self.ended_executions.insert(execution.id.clone());
            }
        }
        // A wait item's owner state follows the evidence its decisions
        // covered (D-0007 §4), restated here without the reducer's helper: an
        // item holding active evidence shows an action exactly when that
        // action's decisions together cover every active witness (evidence
        // the owner never handled keeps it actionable); an item whose
        // evidence all ended keeps every decision made on it. A decision is
        // never dropped.
        for scope in state.waits.values() {
            for decision in &scope.owner_decisions {
                self.wait_decisions.insert((scope.key.clone(), decision.command_id.clone()));
            }
            for (index, episode) in wait::partition(scope) {
                let item = scope
                    .episodes
                    .iter()
                    .find(|e| e.index == index)
                    .and_then(|e| e.attention_id.as_ref())
                    .and_then(|id| state.attention.get(id));
                let Some(item) = item else { continue };
                let shown = [
                    item.acknowledged(),
                    item.resolutions.iter().any(|c| c.kind == ResolutionKind::Owner),
                    item.snoozed_until_ms.is_some(),
                ];
                for (name, shown) in ["Acknowledge", "Resolve", "Snooze"].into_iter().zip(shown) {
                    let decisions: Vec<_> =
                        scope.owner_decisions.iter().filter(|d| action_name(&d.action) == name).collect();
                    let active = !episode.active.is_empty() || episode.unordered > 0;
                    let covered = if active {
                        episode.active.iter().all(|p| decisions.iter().any(|d| d.positives.contains(p)))
                            && (episode.unordered == 0 || decisions.iter().any(|d| d.unordered >= episode.unordered))
                    } else {
                        decisions.iter().any(|d| !d.positives.is_disjoint(&episode.positives))
                    };
                    if covered != shown {
                        self.violations.push(format!(
                            "wait episode {index} of {}: {name} shown={shown}, covered={covered}",
                            scope.key
                        ));
                    }
                }
            }
        }
        for (key, command) in &self.wait_decisions {
            if !state.waits.get(key).is_some_and(|w| w.owner_decisions.iter().any(|d| &d.command_id == command)) {
                self.violations.push(format!("owner decision {command} lost"));
            }
        }
        for item in state
            .attention
            .values()
            .filter(|item| !matches!(item.scope, AttentionScope::SessionWaitCategory { .. }))
        {
            let owner = item.resolutions.iter().any(|c| c.kind == ResolutionKind::Owner);
            if self.owner_resolved.contains(&item.id) && !owner {
                self.violations.push(format!("owner resolution of {} lost", item.id));
            }
            if owner {
                self.owner_resolved.insert(item.id.clone());
            }
            if self.acknowledged.contains(&item.id) && !item.acknowledged() {
                self.violations.push(format!("acknowledgement of {} lost", item.id));
            }
            if item.acknowledged() {
                self.acknowledged.insert(item.id.clone());
            }
        }
        let mut keys = BTreeSet::new();
        for session in state.sessions.values() {
            if !keys.insert((session.namespace_id.clone(), session.native_session_id.clone())) {
                self.violations
                    .push(format!("duplicate session {}", session.native_session_id));
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunReport {
    pub semantic_hash: String,
    pub state_hash: String,
    pub violations: Vec<String>,
    pub receipts: Vec<Receipt>,
    pub owner_failures: Vec<String>,
}

/// Delivers `order` (indices into the scenario's steps) through `runner`.
pub fn run<R: Admit>(scenario: &Scenario, order: &[usize], runner: &mut R) -> RunReport {
    run_observed(scenario, order, runner, |_| Vec::new())
}

/// As `run`, calling `after_step` after every delivered step; any problems
/// it returns are reported as violations.
pub fn run_observed<R: Admit>(
    scenario: &Scenario,
    order: &[usize],
    runner: &mut R,
    mut after_step: impl FnMut(&R) -> Vec<String>,
) -> RunReport {
    let mut monitor = Monitor::default();
    let mut receipts = Vec::new();
    let mut owner_failures = Vec::new();
    for &index in order {
        let step = &scenario.steps[index];
        let before = runner.state().clone();
        let receipt = match step {
            Step::Observe(envelope) => runner.observe(envelope),
            Step::Owner(OwnerStep {
                command_id,
                target,
                action,
                at_ms,
            }) => match target_attention(runner.state(), target) {
                Some(attention_id) => runner.owner(
                    &OwnerCommand {
                        command_id: command_id.clone(),
                        attention_id,
                        expected_revision: None,
                        action: action.clone(),
                    },
                    *at_ms,
                ),
                None => {
                    owner_failures.push(format!("{command_id}: target not found"));
                    Receipt {
                        status: RecordStatus::NotAccepted,
                        cursor: None,
                        reason: Some("TARGET_NOT_FOUND".into()),
                    }
                }
            },
        };
        monitor.after(step, &before, runner.state());
        monitor.violations.extend(after_step(runner));
        receipts.push(receipt);
    }
    RunReport {
        semantic_hash: threadspace_state_engine::semantic::semantic_hash(runner.state()),
        state_hash: threadspace_state_engine::hash::state_hash(runner.state()),
        violations: monitor.violations,
        receipts,
        owner_failures,
    }
}

/// Replays journaled entries into a fresh engine: the exact replay path.
pub fn replay(entries: &[JournalEntry]) -> Engine {
    let mut engine = Engine::empty();
    for entry in entries {
        engine.apply(entry);
    }
    engine
}
