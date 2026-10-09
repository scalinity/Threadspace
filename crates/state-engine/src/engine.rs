//! The engine: canonical state plus secondary indexes, and the one entry
//! point that reduces an admitted journal entry (SPEC §5.5).
//!
//! `apply` is deterministic over (state, entry): it reads no clock,
//! randomness, process, filesystem or provider state. Indexes are derived
//! from the state and rebuilt on load, so they are never a second truth.

use std::collections::{BTreeMap, BTreeSet};

use threadspace_contracts::canonical::fact::{Delivery, JournalEntry};
use threadspace_contracts::canonical::records::{
    ActivityRecord, ActorRecord, AttentionRecord, CanonicalState, ExactRequestRecord,
    ExecutionRecord, HumanFrontier, InputRecord, NamespaceRecord, OutboxRecord, ProcessRecord,
    SessionRecord, SourceCoverage, SourceSurfaceRecord, SurfaceBindingRecord, TurnRecord,
    WaitScopeRecord,
};

use crate::REDUCER_VERSION;

/// Secondary indexes over the state, all derived from it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Index {
    pub session_executions: BTreeMap<String, BTreeSet<String>>,
    pub session_turns: BTreeMap<String, BTreeSet<String>>,
    pub session_bindings: BTreeMap<String, BTreeSet<String>>,
    pub execution_bindings: BTreeMap<String, BTreeSet<String>>,
    pub process_executions: BTreeMap<String, BTreeSet<String>>,
    pub turn_waits: BTreeMap<String, BTreeSet<String>>,
    pub turn_requests: BTreeMap<String, BTreeSet<String>>,
    pub session_frontiers: BTreeMap<String, BTreeSet<String>>,
    pub session_inputs: BTreeMap<String, BTreeSet<String>>,
    /// Every intent recorded for an item, whatever its request ID (a migrated
    /// M0 intent keeps its own).
    pub attention_outbox: BTreeMap<String, BTreeSet<String>>,
}

fn link(map: &mut BTreeMap<String, BTreeSet<String>>, from: &str, to: &str) {
    map.entry(from.to_owned()).or_default().insert(to.to_owned());
}

impl Index {
    pub fn build(state: &CanonicalState) -> Self {
        let mut index = Self::default();
        for execution in state.executions.values() {
            index.add_execution(execution);
        }
        for turn in state.turns.values() {
            link(&mut index.session_turns, &turn.session_id, &turn.id);
        }
        for binding in state.bindings.values() {
            index.add_binding(binding);
        }
        for wait in state.waits.values() {
            if let Some(turn) = &wait.turn_id {
                link(&mut index.turn_waits, turn, &wait.key);
            }
        }
        for request in state.requests.values() {
            if let Some(turn) = &request.turn_id {
                link(&mut index.turn_requests, turn, &request.key);
            }
        }
        for frontier in state.frontiers.values() {
            link(&mut index.session_frontiers, &frontier.session_id, &frontier.key);
        }
        for input in state.inputs.values() {
            link(&mut index.session_inputs, &input.session_id, &input.id);
        }
        for outbox in state.outbox.values() {
            link(&mut index.attention_outbox, &outbox.attention_id, &outbox.request_id);
        }
        index
    }

    pub(crate) fn add_execution(&mut self, execution: &ExecutionRecord) {
        link(&mut self.session_executions, &execution.session_id, &execution.id);
        if let Some(process) = &execution.process_id {
            link(&mut self.process_executions, process, &execution.id);
        }
    }

    pub(crate) fn add_binding(&mut self, binding: &SurfaceBindingRecord) {
        link(&mut self.session_bindings, &binding.session_id, &binding.id);
        link(&mut self.execution_bindings, &binding.execution_id, &binding.id);
    }

    pub(crate) fn link_turn(&mut self, session: &str, turn: &str) {
        link(&mut self.session_turns, session, turn);
    }

    pub(crate) fn link_turn_wait(&mut self, turn: &str, wait: &str) {
        link(&mut self.turn_waits, turn, wait);
    }

    pub(crate) fn link_turn_request(&mut self, turn: &str, request: &str) {
        link(&mut self.turn_requests, turn, request);
    }

    pub(crate) fn link_frontier(&mut self, session: &str, frontier: &str) {
        link(&mut self.session_frontiers, session, frontier);
    }

    pub(crate) fn unlink_frontier(&mut self, session: &str, frontier: &str) {
        if let Some(keys) = self.session_frontiers.get_mut(session) {
            keys.remove(frontier);
            if keys.is_empty() {
                self.session_frontiers.remove(session);
            }
        }
    }

    pub(crate) fn link_input(&mut self, session: &str, input: &str) {
        link(&mut self.session_inputs, session, input);
    }

    pub(crate) fn link_outbox(&mut self, attention: &str, request: &str) {
        link(&mut self.attention_outbox, attention, request);
    }
}

/// Every record whose materialized form an entry changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changed {
    pub namespaces: BTreeSet<String>,
    pub sessions: BTreeSet<String>,
    pub actors: BTreeSet<String>,
    pub relations: bool,
    pub processes: BTreeSet<String>,
    pub executions: BTreeSet<String>,
    pub turns: BTreeSet<String>,
    pub inputs: BTreeSet<String>,
    pub activities: BTreeSet<String>,
    pub surfaces: BTreeSet<String>,
    pub bindings: BTreeSet<String>,
    pub waits: BTreeSet<String>,
    pub requests: BTreeSet<String>,
    pub attention: BTreeSet<String>,
    pub outbox: BTreeSet<String>,
    pub frontiers: BTreeSet<String>,
    pub coverage: BTreeSet<String>,
    pub commands: BTreeSet<String>,
}

impl Changed {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// A bounded diagnostic about an applied entry (never state).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub code: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReduceOutput {
    pub changed: Changed,
    /// Outbox intents created PENDING by this entry: post-commit effects.
    pub new_outbox: Vec<String>,
    pub notes: Vec<Note>,
}

/// Each record as it was before the entry first touched it.
#[derive(Debug, Default)]
pub(crate) struct Before {
    pub namespaces: BTreeMap<String, Option<NamespaceRecord>>,
    pub sessions: BTreeMap<String, Option<SessionRecord>>,
    pub actors: BTreeMap<String, Option<ActorRecord>>,
    pub processes: BTreeMap<String, Option<ProcessRecord>>,
    pub executions: BTreeMap<String, Option<ExecutionRecord>>,
    pub turns: BTreeMap<String, Option<TurnRecord>>,
    pub inputs: BTreeMap<String, Option<InputRecord>>,
    pub activities: BTreeMap<String, Option<ActivityRecord>>,
    pub surfaces: BTreeMap<String, Option<SourceSurfaceRecord>>,
    pub bindings: BTreeMap<String, Option<SurfaceBindingRecord>>,
    pub waits: BTreeMap<String, Option<WaitScopeRecord>>,
    pub requests: BTreeMap<String, Option<ExactRequestRecord>>,
    pub attention: BTreeMap<String, Option<AttentionRecord>>,
    pub outbox: BTreeMap<String, Option<OutboxRecord>>,
    pub frontiers: BTreeMap<String, Option<HumanFrontier>>,
    pub coverage: BTreeMap<String, Option<SourceCoverage>>,
}

pub(crate) fn snap<T: Clone>(
    map: &BTreeMap<String, T>,
    before: &mut BTreeMap<String, Option<T>>,
    id: &str,
) {
    if !before.contains_key(id) {
        before.insert(id.to_owned(), map.get(id).cloned());
    }
}

/// Records which entries of `before` really changed, bumping their revision
/// to `cursor` unless `keep_revision` (bindings keep their proof revision).
pub(crate) fn settle<T: Clone + PartialEq>(
    map: &mut BTreeMap<String, T>,
    before: &BTreeMap<String, Option<T>>,
    changed: &mut BTreeSet<String>,
    cursor: i64,
    revision: impl Fn(&mut T) -> &mut i64,
    keep_revision: bool,
) {
    for (id, old) in before {
        let Some(new) = map.get_mut(id) else { continue };
        let differs = match old {
            None => true,
            Some(old) => {
                let mut probe = new.clone();
                let old_revision = {
                    let mut old = old.clone();
                    *revision(&mut old)
                };
                *revision(&mut probe) = old_revision;
                probe != *old
            }
        };
        if differs {
            if !keep_revision {
                *revision(new) = cursor;
            }
            changed.insert(id.clone());
        }
    }
}

/// Records which entries of `before` changed, for records without a revision.
pub(crate) fn settle_plain<T: PartialEq>(
    map: &BTreeMap<String, T>,
    before: &BTreeMap<String, Option<T>>,
    changed: &mut BTreeSet<String>,
) {
    for (id, old) in before {
        if let Some(new) = map.get(id)
            && old.as_ref() != Some(new)
        {
            changed.insert(id.clone());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    pub state: CanonicalState,
    pub(crate) index: Index,
}

impl Default for Engine {
    fn default() -> Self {
        Self::empty()
    }
}

impl Engine {
    pub fn empty() -> Self {
        Self::new(CanonicalState {
            reducer_version: REDUCER_VERSION,
            ..CanonicalState::default()
        })
    }

    pub fn new(state: CanonicalState) -> Self {
        let index = Index::build(&state);
        Self { state, index }
    }

    pub fn index(&self) -> &Index {
        &self.index
    }

    /// Re-derives every record from its stored evidence (a migrated
    /// baseline). Deterministic and side-effect free.
    pub fn rederive(&mut self, cursor: i64, endpoint_id: &str) -> ReduceOutput {
        crate::reduce::rederive(self, cursor, endpoint_id, Delivery::Bootstrap)
    }

    /// Brings a state checkpointed by an earlier reducer to this one: every
    /// record re-derived under the current rules at its last cursor.
    /// Deterministic and side-effect free.
    pub fn upgrade(&mut self, endpoint_id: &str) -> ReduceOutput {
        let cursor = self.state.through_cursor;
        let output = crate::reduce::rederive(self, cursor, endpoint_id, Delivery::Catchup);
        self.state.reducer_version = REDUCER_VERSION;
        output
    }

    /// Reduces one admitted entry. Deterministic and side-effect free.
    pub fn apply(&mut self, entry: &JournalEntry) -> ReduceOutput {
        crate::reduce::apply(self, entry)
    }

    /// Reduces an entry an earlier reducer admitted, replayed only to bring
    /// that reducer's checkpoint to the journal's end before an upgrade.
    /// Its live effects were committed when it was admitted, so a live
    /// entry is reduced as catch-up: eligibility it yields is held, never
    /// fresh live work. The entry itself is unchanged.
    pub fn recover(&mut self, entry: &JournalEntry) -> ReduceOutput {
        if entry.delivery != Delivery::Live {
            return self.apply(entry);
        }
        let entry = JournalEntry {
            delivery: Delivery::Catchup,
            ..entry.clone()
        };
        crate::reduce::apply(self, &entry)
    }
}
