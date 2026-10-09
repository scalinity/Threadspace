//! The canonical reducer (SPEC §5.5, §6, §7).
//!
//! Each fact adds evidence to the records it references (an outcome to a
//! turn's outcome set, a clear barrier to a wait scope, a reason to an
//! execution's end reasons). Afterwards every derived field of each touched
//! record is recomputed from its evidence, in dependency order: process →
//! execution presence → binding validity → turn state → wait episodes →
//! human-follow-up frontier → attention → outbox → session. Because the
//! derived fields depend only on accumulated evidence, valid permutations of
//! the same facts converge (INV-14), and terminal states cannot regress.

use std::collections::{BTreeMap, BTreeSet};

use threadspace_contracts::canonical::causal::{CausalOrder, CausalPoint};
use threadspace_contracts::canonical::fact::{
    ActorRole, AttachedPresence, Delivery, EvidenceClass, ExecutionMode, FactPayload, InputOrigin,
    JournalEntry, ResolvedFact, TurnOutcome, WaitCategory, WaitSignal,
};
use threadspace_contracts::canonical::keys::{NativeActorRef, NativeExecutionRef};
use threadspace_contracts::canonical::records::{
    ActivityRecord, ActorRecord, ActorRelationRecord, AttachObservation, AttentionRecord,
    AttentionScope, CanonicalState, CommandEffect, ExactRequestRecord, ExecutionRecord,
    HumanFrontier, InputRecord, InputSubmission, InventoryObservation, LinkObservation,
    NamespaceRecord, ObserverTier, OutboxRecord, OutboxState, OwnerActionKind, PendingOutcome,
    ProcessImage, ProcessRecord, ResolutionCause, ResolutionKind, RouteRecord, SequenceRange, SessionRecord,
    SourceCoverage, SourceSurfaceRecord, SummaryAuthority, SurfaceBindingRecord,
    TurnIdentityKind, TurnRecord, WaitEpisode, WaitOwnerDecision, WaitScopeRecord,
};
use threadspace_contracts::canonical::command::OwnerAction;
use threadspace_contracts::cursor::{format_cursor, parse_cursor};
use threadspace_contracts::projection::{
    AttentionCategory, ExecutionPresence, NotificationState, ObservationState, TurnState,
};

use crate::causal::compare;
use crate::engine::{Before, Engine, Index, Note, ReduceOutput, settle, settle_plain, snap};
use crate::ids::derived_id;
use crate::profiles;
use crate::{FIXTURE_PROFILES, FIXTURE_PROVIDER, keys, wait};

const PRIORITY_ERROR: u8 = 90;
const PRIORITY_BLOCKED: u8 = 90;
const PRIORITY_APPROVAL: u8 = 80;
const PRIORITY_INPUT: u8 = 70;
const PRIORITY_OUTPUT: u8 = 40;

struct Tx<'a> {
    state: &'a mut threadspace_contracts::canonical::records::CanonicalState,
    index: &'a mut Index,
    entry: &'a JournalEntry,
    before: Before,
    relations_changed: bool,
    commands: BTreeSet<String>,
    /// The first fact of this entry that touched each record.
    trigger: BTreeMap<String, String>,
    notes: Vec<Note>,
    new_outbox: Vec<String>,
}

pub(crate) fn apply(engine: &mut Engine, entry: &JournalEntry) -> ReduceOutput {
    let mut tx = Tx {
        state: &mut engine.state,
        index: &mut engine.index,
        entry,
        before: Before::default(),
        relations_changed: false,
        commands: BTreeSet::new(),
        trigger: BTreeMap::new(),
        notes: Vec::new(),
        new_outbox: Vec::new(),
    };
    tx.coverage();
    for fact in &entry.facts {
        tx.fact(fact);
    }
    tx.derive();
    tx.state.through_cursor = tx.state.through_cursor.max(entry.cursor);
    tx.finish()
}

/// Re-derives every record from its evidence at `cursor`, with no facts: a
/// migrated baseline (bootstrap delivery, so no outbox intents) or a
/// checkpoint from an earlier reducer (catch-up delivery, so an item that
/// is eligible again re-arms its suppressed intent as HELD).
pub(crate) fn rederive(engine: &mut Engine, cursor: i64, endpoint_id: &str, delivery: Delivery) -> ReduceOutput {
    let entry = JournalEntry {
        cursor,
        endpoint_id: endpoint_id.to_owned(),
        observation_id: String::new(),
        source_id: "baseline".to_owned(),
        source_epoch: "baseline".to_owned(),
        source_sequence: None,
        sequence_meaning: None,
        captured_wall_ms: 0,
        delivery,
        facts: Vec::new(),
    };
    let mut tx = Tx {
        state: &mut engine.state,
        index: &mut engine.index,
        entry: &entry,
        before: Before::default(),
        relations_changed: false,
        commands: BTreeSet::new(),
        trigger: BTreeMap::new(),
        notes: Vec::new(),
        new_outbox: Vec::new(),
    };
    let ids = |keys: Vec<&String>| keys.into_iter().cloned().collect::<Vec<_>>();
    for id in ids(tx.state.processes.keys().collect()) {
        tx.touch_process(&id);
    }
    for id in ids(tx.state.executions.keys().collect()) {
        tx.touch_execution(&id);
    }
    for id in ids(tx.state.bindings.keys().collect()) {
        tx.touch_binding(&id);
    }
    for id in ids(tx.state.turns.keys().collect()) {
        tx.touch_turn(&id);
    }
    for id in ids(tx.state.inputs.keys().collect()) {
        tx.touch_input(&id);
    }
    for id in ids(tx.state.waits.keys().collect()) {
        tx.touch_wait(&id);
    }
    for id in ids(tx.state.attention.keys().collect()) {
        tx.touch_attention(&id);
    }
    for id in ids(tx.state.sessions.keys().collect()) {
        tx.touch_session(&id);
    }
    tx.derive();
    tx.state.through_cursor = tx.state.through_cursor.max(cursor);
    tx.finish()
}

/// Adds the evidence-set part of a fact (D-0010) to the record it names:
/// attach, link and submission reports, and host-read outcomes. The reducer
/// and the reconstruction of an earlier reducer's checkpoint both use it, so
/// both hold exactly the same sets. False when the fact is none of those or
/// its record does not exist.
pub fn record_evidence(state: &mut CanonicalState, fact: &ResolvedFact) -> bool {
    let refs = &fact.refs;
    let observation_id = fact.observation_id.clone();
    let fact_index = fact.fact_index;
    let point = fact.causal.clone();
    match &fact.payload {
        FactPayload::ExecutionAttached {
            mode,
            presence,
            native_runtime_id,
            controlling_device,
        } => refs
            .execution_id
            .as_ref()
            .and_then(|id| state.executions.get_mut(id))
            .is_some_and(|execution| {
                execution.attachments.insert(AttachObservation {
                    observation_id,
                    fact_index,
                    point,
                    provenance: fact.provenance,
                    mode: *mode,
                    presence: *presence,
                    native_runtime_id: native_runtime_id.clone(),
                    controlling_device: *controlling_device,
                });
                true
            }),
        FactPayload::ObservationLinkChanged {
            link,
            qualified,
            version,
        } => refs
            .session_id
            .as_ref()
            .and_then(|id| state.sessions.get_mut(id))
            .is_some_and(|session| {
                session.links.insert(LinkObservation {
                    observation_id,
                    fact_index,
                    point,
                    provenance: fact.provenance,
                    process_id: refs.process_id.clone(),
                    qualified: *qualified,
                    version: version.clone(),
                    link: link.clone(),
                });
                true
            }),
        FactPayload::InputSubmitted { origin, submission } => refs
            .input_id
            .as_ref()
            .and_then(|id| state.inputs.get_mut(id))
            .is_some_and(|input| {
                input.submissions.insert(InputSubmission {
                    observation_id,
                    fact_index,
                    origin: *origin,
                    point: submission.clone().or(point),
                    active_turn_id: refs.turn_id.clone(),
                });
                true
            }),
        FactPayload::TurnOutcomeObserved { outcome, reason, .. } if fact.provenance == EvidenceClass::HostRead => refs
            .turn_id
            .as_ref()
            .and_then(|id| state.turns.get_mut(id))
            .is_some_and(|turn| {
                turn.pending_outcomes.insert(PendingOutcome {
                    observation_id,
                    fact_index,
                    outcome: *outcome,
                    reason: reason.clone(),
                    point,
                    process_id: refs.process_id.clone(),
                });
                true
            }),
        _ => false,
    }
}

/// The items no other item's point causally follows; with unordered
/// evidence, all of them (D-0010).
fn latest<T>(items: Vec<&T>, point: impl Fn(&T) -> Option<&CausalPoint>) -> Vec<&T> {
    items
        .iter()
        .copied()
        .filter(|a| {
            !items.iter().any(|b| match (point(a), point(b)) {
                (Some(p), Some(q)) => compare(p, q) == CausalOrder::Before,
                _ => false,
            })
        })
        .collect()
}

/// The one value the items agree on and whether they disagree: `(None,
/// true)` for disagreement, `(None, false)` when none names a value.
fn agreed<T: Ord + Clone>(values: impl IntoIterator<Item = T>) -> (Option<T>, bool) {
    let distinct: BTreeSet<T> = values.into_iter().collect();
    match distinct.len() {
        0 => (None, false),
        1 => (distinct.into_iter().next(), false),
        _ => (None, true),
    }
}

/// Disagreeing presence settles on the least live (D-0010).
fn presence_rank(presence: AttachedPresence) -> u8 {
    match presence {
        AttachedPresence::Live => 0,
        AttachedPresence::Parked => 1,
        AttachedPresence::Detached => 2,
    }
}

/// Disagreeing link reports settle on the most degraded (D-0010).
fn link_rank(link: &ObservationState) -> u8 {
    match link {
        ObservationState::Current => 0,
        ObservationState::Unknown => 1,
        ObservationState::Stale => 2,
        ObservationState::Conflict => 3,
        ObservationState::Disconnected => 4,
    }
}

const SUPPRESSED_BEFORE_SUBMISSION: &str = "ineligible before submission";
const WAIT_SUPERSEDED: &str = "wait episode superseded by an earlier clear";

fn severity(outcome: TurnOutcome) -> u8 {
    match outcome {
        TurnOutcome::Completed => 0,
        TurnOutcome::Interrupted => 1,
        TurnOutcome::Refused => 2,
        TurnOutcome::Failed => 3,
    }
}

/// The state of the causally latest terminal turn: one whose evidence no
/// other terminal turn's evidence follows. Turns that cannot be ordered
/// (other sources, no sequence) settle by severity, so the result never
/// depends on arrival order.
fn latest_settled_state(turns: &[&TurnRecord]) -> TurnState {
    let settled: Vec<&TurnRecord> = turns.iter().copied().filter(|t| !t.outcomes.is_empty()).collect();
    let followed = |t: &TurnRecord| {
        settled.iter().any(|u| {
            u.id != t.id
                && t.output_points
                    .iter()
                    .any(|p| u.output_points.iter().any(|q| compare(p, q) == CausalOrder::Before))
        })
    };
    let latest: Vec<&TurnRecord> = settled.iter().copied().filter(|t| !followed(t)).collect();
    let candidates = if latest.is_empty() { &settled } else { &latest };
    candidates
        .iter()
        .flat_map(|t| t.outcomes.iter().copied())
        .max_by_key(|o| severity(*o))
        .map_or(TurnState::Unknown, outcome_state)
}

fn outcome_state(outcome: TurnOutcome) -> TurnState {
    match outcome {
        TurnOutcome::Completed => TurnState::Completed,
        TurnOutcome::Interrupted => TurnState::Interrupted,
        TurnOutcome::Failed => TurnState::Failed,
        TurnOutcome::Refused => TurnState::Refused,
    }
}

fn wait_attention(category: WaitCategory) -> (AttentionCategory, u8) {
    match category {
        WaitCategory::Approval => (AttentionCategory::ApprovalRequired, PRIORITY_APPROVAL),
        WaitCategory::Input => (AttentionCategory::InputRequired, PRIORITY_INPUT),
        WaitCategory::JobBlocked => (AttentionCategory::Blocked, PRIORITY_BLOCKED),
    }
}

fn wait_kind(category: WaitCategory) -> &'static str {
    match category {
        WaitCategory::Approval => "APPROVAL",
        WaitCategory::Input => "INPUT",
        WaitCategory::JobBlocked => "JOB_BLOCKED",
    }
}

fn is_native_resolution(kind: ResolutionKind) -> bool {
    !matches!(kind, ResolutionKind::Owner)
}

/// True when an image other than `proven` is observed causally after the
/// proof. Monotone in the evidence, so a lost proof is never revived. A
/// proof without a causal point counts any observation of `proven` that
/// another image follows.
fn replaced_after(images: &BTreeSet<ProcessImage>, proven: &str, proof: Option<&CausalPoint>) -> bool {
    let after = |earlier: &CausalPoint| {
        images.iter().any(|b| {
            b.executable != proven && b.point.as_ref().is_some_and(|q| compare(earlier, q) == CausalOrder::Before)
        })
    };
    match proof {
        Some(proof) => after(proof),
        None => images.iter().filter(|a| a.executable == proven).filter_map(|a| a.point.as_ref()).any(after),
    }
}

/// The image of the one observation every other observation precedes; with
/// a single image, that image. Unordered replacements leave it unknown.
fn current_image(images: &BTreeSet<ProcessImage>) -> Option<String> {
    let distinct: BTreeSet<&String> = images.iter().map(|i| &i.executable).collect();
    if distinct.len() == 1 {
        return distinct.into_iter().next().cloned();
    }
    images
        .iter()
        .find(|candidate| {
            images.iter().all(|other| {
                other.executable == candidate.executable
                    || match (&other.point, &candidate.point) {
                        (Some(o), Some(c)) => compare(o, c) == CausalOrder::Before,
                        _ => false,
                    }
            })
        })
        .map(|image| image.executable.clone())
}

/// Inserts one sequence into a set of merged inclusive ranges.
fn add_sequence(ranges: &mut Vec<SequenceRange>, value: i64) {
    let mut spans: Vec<(i64, i64)> = ranges
        .iter()
        .filter_map(|r| Some((parse_cursor(&r.first)?, parse_cursor(&r.last)?)))
        .collect();
    spans.push((value, value));
    spans.sort_unstable();
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (first, last) in spans {
        match merged.last_mut() {
            Some((_, end)) if first <= end.saturating_add(1) => *end = (*end).max(last),
            _ => merged.push((first, last)),
        }
    }
    *ranges = merged
        .into_iter()
        .map(|(first, last)| SequenceRange {
            first: format_cursor(first),
            last: format_cursor(last),
        })
        .collect();
}

fn gaps_between(ranges: &[SequenceRange]) -> Vec<SequenceRange> {
    ranges
        .windows(2)
        .filter_map(|pair| {
            let end = parse_cursor(&pair[0].last)?;
            let next = parse_cursor(&pair[1].first)?;
            (next > end + 1).then(|| SequenceRange {
                first: format_cursor(end + 1),
                last: format_cursor(next - 1),
            })
        })
        .collect()
}

impl Tx<'_> {
    fn cursor(&self) -> i64 {
        self.entry.cursor
    }

    fn mark(&mut self, id: &str, fact: &ResolvedFact) {
        self.trigger
            .entry(id.to_owned())
            .or_insert_with(|| fact.fact_id.clone());
    }

    fn note(&mut self, code: &'static str, detail: impl Into<String>) {
        self.notes.push(Note {
            code,
            detail: detail.into(),
        });
    }

    // ------------------------------------------------------------ touching

    fn touch_session(&mut self, id: &str) {
        snap(&self.state.sessions, &mut self.before.sessions, id);
    }

    fn touch_execution(&mut self, id: &str) {
        snap(&self.state.executions, &mut self.before.executions, id);
    }

    fn touch_binding(&mut self, id: &str) {
        snap(&self.state.bindings, &mut self.before.bindings, id);
    }

    fn touch_turn(&mut self, id: &str) {
        snap(&self.state.turns, &mut self.before.turns, id);
    }

    fn touch_input(&mut self, id: &str) {
        snap(&self.state.inputs, &mut self.before.inputs, id);
    }

    fn touch_activity(&mut self, id: &str) {
        snap(&self.state.activities, &mut self.before.activities, id);
    }

    fn touch_wait(&mut self, id: &str) {
        snap(&self.state.waits, &mut self.before.waits, id);
    }

    fn touch_request(&mut self, id: &str) {
        snap(&self.state.requests, &mut self.before.requests, id);
    }

    fn touch_attention(&mut self, id: &str) {
        snap(&self.state.attention, &mut self.before.attention, id);
    }

    fn touch_outbox(&mut self, id: &str) {
        snap(&self.state.outbox, &mut self.before.outbox, id);
    }

    fn touch_frontier(&mut self, id: &str) {
        snap(&self.state.frontiers, &mut self.before.frontiers, id);
    }

    fn touch_process(&mut self, id: &str) {
        snap(&self.state.processes, &mut self.before.processes, id);
    }

    fn touch_actor(&mut self, id: &str) {
        snap(&self.state.actors, &mut self.before.actors, id);
    }

    // ------------------------------------------------------------ coverage

    /// Sequence coverage of the entry's source epoch (SPEC §5.4). A gap is a
    /// coverage fact about that source only.
    fn coverage(&mut self) {
        let entry = self.entry;
        let (Some(meaning), Some(sequence)) = (
            entry.sequence_meaning,
            entry.source_sequence.as_deref().and_then(parse_cursor),
        ) else {
            return;
        };
        let key = keys::coverage(&entry.source_id, &entry.source_epoch);
        snap(&self.state.coverage, &mut self.before.coverage, &key);
        let record = self
            .state
            .coverage
            .entry(key.clone())
            .or_insert_with(|| SourceCoverage {
                key,
                source_id: entry.source_id.clone(),
                source_epoch: entry.source_epoch.clone(),
                meaning,
                seen: Vec::new(),
                gaps: Vec::new(),
                reported_gaps: BTreeSet::new(),
                revision: entry.cursor,
            });
        add_sequence(&mut record.seen, sequence);
        record.gaps = gaps_between(&record.seen);
    }

    // ------------------------------------------------------------ ensuring

    fn ensure_records(&mut self, fact: &ResolvedFact) {
        let refs = &fact.refs;
        let native = &fact.native;
        let cursor = self.cursor();
        if let (Some(namespace_id), Some(session)) = (&refs.namespace_id, &native.session)
            && !self.state.namespaces.contains_key(namespace_id)
        {
            snap(
                &self.state.namespaces,
                &mut self.before.namespaces,
                namespace_id,
            );
            self.state.namespaces.insert(
                namespace_id.clone(),
                NamespaceRecord {
                    id: namespace_id.clone(),
                    provider: session.provider.clone(),
                    endpoint_id: self.entry.endpoint_id.clone(),
                    profile_ref: session.profile_ref.clone(),
                },
            );
        }
        if let (Some(session_id), Some(namespace_id), Some(session)) =
            (&refs.session_id, &refs.namespace_id, &native.session)
            && !self.state.sessions.contains_key(session_id)
        {
            self.touch_session(session_id);
            let fixture = session.provider == FIXTURE_PROVIDER
                && FIXTURE_PROFILES.contains(&session.profile_ref.as_str());
            self.state.sessions.insert(
                session_id.clone(),
                SessionRecord {
                    id: session_id.clone(),
                    namespace_id: namespace_id.clone(),
                    native_session_id: session.native_session_id.clone(),
                    record_state:
                        threadspace_contracts::canonical::fact::SessionRecordState::Known,
                    display_name: None,
                    start_sources: BTreeSet::new(),
                    links: BTreeSet::new(),
                    link: None,
                    inventory: None,
                    last_route: None,
                    fixture,
                    execution_presence: ExecutionPresence::Unknown,
                    observation: ObservationState::Unknown,
                    turn_state: TurnState::Unknown,
                    link_conflict: false,
                    observer_tier: None,
                    observer_version: None,
                    created_cursor: cursor,
                    revision: cursor,
                },
            );
        }
        if let Some(session_id) = &refs.session_id
            && self.state.sessions.contains_key(session_id)
        {
            self.touch_session(session_id);
        }
        for (actor_id, actor) in [
            (&refs.actor_id, native.actor.clone().unwrap_or(NativeActorRef::Principal)),
            (
                &refs.related_actor_id,
                native
                    .related_actor
                    .clone()
                    .unwrap_or(NativeActorRef::Principal),
            ),
        ] {
            if let (Some(actor_id), Some(session_id)) = (actor_id, &refs.session_id)
                && !self.state.actors.contains_key(actor_id)
            {
                self.touch_actor(actor_id);
                let role = match actor {
                    NativeActorRef::Principal => ActorRole::Principal,
                    NativeActorRef::Agent { .. } => ActorRole::Subordinate,
                };
                self.state.actors.insert(
                    actor_id.clone(),
                    ActorRecord {
                        id: actor_id.clone(),
                        session_id: session_id.clone(),
                        native: actor,
                        role,
                        agent_types: BTreeSet::new(),
                        runs_ended: 0,
                        created_cursor: cursor,
                        revision: cursor,
                    },
                );
            }
        }
        if let (Some(process_id), Some(key)) = (&refs.process_id, &native.process)
            && !self.state.processes.contains_key(process_id)
        {
            self.touch_process(process_id);
            self.state.processes.insert(
                process_id.clone(),
                ProcessRecord {
                    id: process_id.clone(),
                    key: key.clone(),
                    images: BTreeSet::new(),
                    current_executable: None,
                    exited: false,
                    created_cursor: cursor,
                    revision: cursor,
                },
            );
        }
        if let (
            Some(execution_id),
            Some(session_id),
            Some(actor_id),
            Some(NativeExecutionRef::Activation { activation_ref }),
        ) = (
            &refs.execution_id,
            &refs.session_id,
            &refs.actor_id,
            &native.execution,
        ) && !self.state.executions.contains_key(execution_id)
        {
            self.touch_execution(execution_id);
            let activation = self
                .index
                .session_executions
                .get(session_id)
                .map_or(0, |ids| {
                    ids.iter()
                        .filter_map(|id| self.state.executions.get(id))
                        .map(|e| e.activation)
                        .max()
                        .unwrap_or(0)
                })
                + 1;
            let record = ExecutionRecord {
                id: execution_id.clone(),
                session_id: session_id.clone(),
                actor_id: actor_id.clone(),
                activation_ref: activation_ref.clone(),
                process_id: refs.process_id.clone(),
                activation,
                attachments: BTreeSet::new(),
                mode: None,
                attached: None,
                native_runtime_id: None,
                controlling_device: None,
                end_reasons: BTreeSet::new(),
                surface_status: None,
                attachment_conflict: false,
                presence: ExecutionPresence::Unknown,
                started_cursor: None,
                ended_cursor: None,
                created_cursor: cursor,
                revision: cursor,
            };
            self.index.add_execution(&record);
            self.state.executions.insert(execution_id.clone(), record);
        }
        if let Some(execution_id) = &refs.execution_id
            && self.state.executions.contains_key(execution_id)
        {
            self.touch_execution(execution_id);
            // A process named later than the execution's first fact joins it.
            if let Some(process_id) = &refs.process_id
                && let Some(execution) = self.state.executions.get_mut(execution_id)
                && execution.process_id.is_none()
            {
                execution.process_id = Some(process_id.clone());
                let record = execution.clone();
                self.index.add_execution(&record);
            }
        }
        if let (Some(turn_id), Some(session_id), Some(actor_id), Some(native_turn)) =
            (&refs.turn_id, &refs.session_id, &refs.actor_id, &native.turn)
            && !self.state.turns.contains_key(turn_id)
        {
            self.touch_turn(turn_id);
            let owner_facing = self
                .state
                .actors
                .get(actor_id)
                .is_some_and(|actor| actor.native == NativeActorRef::Principal);
            self.index.link_turn(session_id, turn_id);
            self.state.turns.insert(
                turn_id.clone(),
                TurnRecord {
                    id: turn_id.clone(),
                    session_id: session_id.clone(),
                    actor_id: actor_id.clone(),
                    execution_ids: BTreeSet::new(),
                    native_turn_id: Some(native_turn.clone()),
                    identity_kind: TurnIdentityKind::Native,
                    owner_facing,
                    started: false,
                    stepped: false,
                    activity_seen: false,
                    queued_inputs: BTreeSet::new(),
                    started_by_inputs: BTreeSet::new(),
                    outcomes: BTreeSet::new(),
                    outcome_reasons: BTreeSet::new(),
                    pending_outcomes: BTreeSet::new(),
                    output_ready: false,
                    output_points: BTreeSet::new(),
                    summary: None,
                    state: TurnState::Unknown,
                    outcome_conflict: false,
                    created_cursor: cursor,
                    revision: cursor,
                },
            );
        }
        if let Some(turn_id) = &refs.turn_id
            && self.state.turns.contains_key(turn_id)
        {
            self.touch_turn(turn_id);
            if let (Some(execution_id), Some(turn)) =
                (&refs.execution_id, self.state.turns.get_mut(turn_id))
            {
                turn.execution_ids.insert(execution_id.clone());
            }
        }
        if let (Some(input_id), Some(session_id), Some(actor_id), Some(native_input)) =
            (&refs.input_id, &refs.session_id, &refs.actor_id, &native.input)
            && !self.state.inputs.contains_key(input_id)
        {
            self.touch_input(input_id);
            self.index.link_input(session_id, input_id);
            self.state.inputs.insert(
                input_id.clone(),
                InputRecord {
                    id: input_id.clone(),
                    session_id: session_id.clone(),
                    actor_id: actor_id.clone(),
                    native_key: native_input.clone(),
                    submissions: BTreeSet::new(),
                    origin: None,
                    submission: None,
                    active_turn_id: None,
                    acceptances: BTreeSet::new(),
                    rejections: BTreeSet::new(),
                    started_turns: BTreeSet::new(),
                    created_cursor: cursor,
                    revision: cursor,
                },
            );
        }
        if let Some(input_id) = &refs.input_id
            && self.state.inputs.contains_key(input_id)
        {
            self.touch_input(input_id);
        }
        if let (Some(activity_id), Some(session_id), Some(actor_id), Some(occurrence)) = (
            &refs.activity_id,
            &refs.session_id,
            &refs.actor_id,
            &native.activity,
        ) && !self.state.activities.contains_key(activity_id)
        {
            self.touch_activity(activity_id);
            self.state.activities.insert(
                activity_id.clone(),
                ActivityRecord {
                    id: activity_id.clone(),
                    session_id: session_id.clone(),
                    actor_id: actor_id.clone(),
                    turn_id: refs.turn_id.clone(),
                    native_occurrence_id: occurrence.clone(),
                    tool_categories: BTreeSet::new(),
                    proposed: false,
                    started: false,
                    finished: BTreeSet::new(),
                    permission_checked: false,
                    created_cursor: cursor,
                    revision: cursor,
                },
            );
        }
        if let Some(activity_id) = &refs.activity_id
            && self.state.activities.contains_key(activity_id)
        {
            self.touch_activity(activity_id);
        }
        if let (Some(surface_id), Some(surface)) = (&refs.surface_id, &native.surface)
            && !self.state.surfaces.contains_key(surface_id)
        {
            snap(&self.state.surfaces, &mut self.before.surfaces, surface_id);
            self.state.surfaces.insert(
                surface_id.clone(),
                SourceSurfaceRecord {
                    id: surface_id.clone(),
                    endpoint_id: self.entry.endpoint_id.clone(),
                    native: surface.clone(),
                    created_cursor: cursor,
                    revision: cursor,
                },
            );
        }
        for id in [
            &refs.session_id,
            &refs.execution_id,
            &refs.turn_id,
            &refs.input_id,
            &refs.activity_id,
            &refs.binding_id,
            &refs.attention_id,
        ]
        .into_iter()
        .flatten()
        {
            self.mark(id, fact);
        }
    }

    fn ensure_binding(&mut self, fact: &ResolvedFact) -> Option<String> {
        let refs = &fact.refs;
        let (Some(binding_id), Some(session_id), Some(execution_id), Some(surface_id)) = (
            &refs.binding_id,
            &refs.session_id,
            &refs.execution_id,
            &refs.surface_id,
        ) else {
            return None;
        };
        self.touch_binding(binding_id);
        if !self.state.bindings.contains_key(binding_id) {
            let record = SurfaceBindingRecord {
                id: binding_id.clone(),
                session_id: session_id.clone(),
                execution_id: execution_id.clone(),
                surface_id: surface_id.clone(),
                method: None,
                executable_identity: None,
                window_hint: None,
                tab_hint: None,
                proof: serde_json::Value::Null,
                evidence_observation: None,
                proof_point: None,
                invalidations: BTreeSet::new(),
                valid: false,
                invalidation_reason: None,
                recorded_cursor: None,
                invalidated_cursor: None,
                created_cursor: self.cursor(),
                revision: self.cursor(),
            };
            self.index.add_binding(&record);
            self.state.bindings.insert(binding_id.clone(), record);
        }
        Some(binding_id.clone())
    }

    // ------------------------------------------------------------ facts

    #[allow(clippy::too_many_lines)]
    fn fact(&mut self, fact: &ResolvedFact) {
        self.ensure_records(fact);
        let refs = &fact.refs;
        let cursor = self.cursor();
        match &fact.payload {
            FactPayload::SessionIdentified {
                display_name,
                start_source,
            } => {
                if let Some(session) = refs.session_id.as_ref().and_then(|id| self.state.sessions.get_mut(id)) {
                    if display_name.is_some() {
                        session.display_name.clone_from(display_name);
                    }
                    if let Some(source) = start_source {
                        session.start_sources.insert(source.clone());
                    }
                }
            }
            FactPayload::SessionRecordChanged { record_state } => {
                if let Some(session) = refs.session_id.as_ref().and_then(|id| self.state.sessions.get_mut(id)) {
                    session.record_state = *record_state;
                }
            }
            FactPayload::ExecutionAttached { .. } => {
                // Mode and presence are derived from the attach observations.
                if record_evidence(self.state, fact)
                    && let Some(execution) = refs.execution_id.as_ref().and_then(|id| self.state.executions.get_mut(id))
                {
                    execution.started_cursor.get_or_insert(cursor);
                }
            }
            FactPayload::ExecutionEnded { reason } => {
                if let Some(execution) = refs.execution_id.as_ref().and_then(|id| self.state.executions.get_mut(id)) {
                    execution.end_reasons.insert(reason.clone());
                }
            }
            FactPayload::ProcessObserved {
                executable_identity,
            } => {
                if let Some(id) = &refs.process_id {
                    self.touch_process(id);
                    if let Some(process) = self.state.processes.get_mut(id) {
                        process.images.insert(ProcessImage {
                            executable: executable_identity.clone(),
                            point: fact.causal.clone(),
                        });
                        process.current_executable = current_image(&process.images);
                    }
                }
            }
            FactPayload::ProcessExitObserved {} => {
                if let Some(id) = &refs.process_id {
                    self.touch_process(id);
                    if let Some(process) = self.state.processes.get_mut(id) {
                        process.exited = true;
                    }
                }
            }
            // The link, and an input's origin, submission point and active
            // turn, are derived from their reports.
            FactPayload::ObservationLinkChanged { .. } | FactPayload::InputSubmitted { .. } => {
                record_evidence(self.state, fact);
            }
            FactPayload::InputAccepted { proof } => {
                if let Some(input) = refs.input_id.as_ref().and_then(|id| self.state.inputs.get_mut(id)) {
                    input.acceptances.insert(*proof);
                }
            }
            FactPayload::InputRejected { reason } => {
                if let Some(input) = refs.input_id.as_ref().and_then(|id| self.state.inputs.get_mut(id)) {
                    input.rejections.insert(reason.clone());
                }
            }
            FactPayload::TurnStarted {} => {
                if let Some(turn_id) = &refs.turn_id {
                    if let Some(turn) = self.state.turns.get_mut(turn_id) {
                        turn.started = true;
                        if let Some(input) = &refs.input_id {
                            turn.started_by_inputs.insert(input.clone());
                        }
                    }
                    if let Some(input) = refs.input_id.as_ref().and_then(|id| self.state.inputs.get_mut(id)) {
                        input.started_turns.insert(turn_id.clone());
                    }
                }
            }
            FactPayload::TurnStepObserved {} => {
                if let Some(turn) = refs.turn_id.as_ref().and_then(|id| self.state.turns.get_mut(id)) {
                    turn.stepped = true;
                }
            }
            FactPayload::ResponseBoundaryObserved { .. } => {
                // A response boundary may be continued or vetoed: it changes no
                // turn outcome and no execution presence (INV-07).
            }
            FactPayload::OutputReady { summary } => {
                if let Some(turn) = refs.turn_id.as_ref().and_then(|id| self.state.turns.get_mut(id)) {
                    turn.output_ready = true;
                    if let Some(point) = &fact.causal {
                        turn.output_points.insert(point.clone());
                    }
                    if turn.summary.is_none() {
                        turn.summary.clone_from(summary);
                    }
                }
            }
            FactPayload::TurnOutcomeObserved { .. } if fact.provenance == EvidenceClass::HostRead => {
                // Retained; applied once corroborated (`derive_turn`).
                record_evidence(self.state, fact);
            }
            FactPayload::TurnOutcomeObserved {
                outcome,
                reason,
                summary,
            } => {
                if let Some(turn) = refs.turn_id.as_ref().and_then(|id| self.state.turns.get_mut(id)) {
                    turn.outcomes.insert(*outcome);
                    if let Some(reason) = reason {
                        turn.outcome_reasons.insert(reason.clone());
                    }
                    if let Some(point) = &fact.causal {
                        turn.output_points.insert(point.clone());
                    }
                    if turn.summary.is_none() {
                        turn.summary.clone_from(summary);
                    }
                }
            }
            FactPayload::ActivityProposed { tool_category }
            | FactPayload::ActivityStarted { tool_category }
            | FactPayload::ActivityFinished { tool_category, .. } => {
                if let Some(activity) = refs.activity_id.as_ref().and_then(|id| self.state.activities.get_mut(id)) {
                    activity.tool_categories.insert(tool_category.clone());
                    if activity.turn_id.is_none() {
                        activity.turn_id.clone_from(&refs.turn_id);
                    }
                    match &fact.payload {
                        FactPayload::ActivityProposed { .. } => activity.proposed = true,
                        FactPayload::ActivityStarted { .. } => activity.started = true,
                        FactPayload::ActivityFinished { result, .. } => {
                            activity.finished.insert(*result);
                        }
                        _ => {}
                    }
                }
                if let Some(turn) = refs.turn_id.as_ref().and_then(|id| self.state.turns.get_mut(id)) {
                    turn.activity_seen = true;
                }
            }
            FactPayload::PermissionCheckObserved { .. } => {
                // A preflight occurred; it is not a human wait (SPEC §11.5).
                if let Some(activity) = refs.activity_id.as_ref().and_then(|id| self.state.activities.get_mut(id)) {
                    activity.permission_checked = true;
                }
            }
            FactPayload::WaitStateObserved {
                category,
                signal,
                subtype,
                generation,
            } => self.wait(fact, *category, *signal, subtype.as_ref(), generation.as_ref()),
            FactPayload::RequestResolved {} => {
                if let (Some(session_id), Some(request)) = (&refs.session_id, &refs.request) {
                    let key = self.request_record(session_id, request, fact);
                    if let Some(record) = self.state.requests.get_mut(&key) {
                        record.resolved = true;
                    }
                }
            }
            FactPayload::ActorIdentified { role, agent_type } => {
                if let Some(actor_id) = &refs.actor_id {
                    self.touch_actor(actor_id);
                    if let Some(actor) = self.state.actors.get_mut(actor_id) {
                        if let Some(agent_type) = agent_type {
                            actor.agent_types.insert(agent_type.clone());
                        }
                        // Role only strengthens for a subordinate native actor;
                        // a child is never promoted to the principal (INV-09).
                        if *role == ActorRole::Teammate
                            && matches!(actor.native, NativeActorRef::Agent { .. })
                        {
                            actor.role = ActorRole::Teammate;
                        } else if *role == ActorRole::Principal
                            && matches!(actor.native, NativeActorRef::Agent { .. })
                        {
                            self.note("ACTOR_PROMOTION_REFUSED", actor_id.clone());
                        }
                    }
                }
            }
            FactPayload::ActorRelationObserved { relation } => {
                if let (Some(actor_id), Some(related)) = (&refs.actor_id, &refs.related_actor_id) {
                    self.relations_changed |= self.state.relations.insert(ActorRelationRecord {
                        actor_id: actor_id.clone(),
                        related_actor_id: related.clone(),
                        relation: *relation,
                    });
                }
            }
            FactPayload::ActorRunEnded { .. } => {
                if let Some(actor_id) = &refs.actor_id {
                    self.touch_actor(actor_id);
                    if let Some(actor) = self.state.actors.get_mut(actor_id) {
                        actor.runs_ended = actor.runs_ended.saturating_add(1);
                    }
                }
            }
            FactPayload::SurfaceBindingRecorded { proof } => {
                if let Some(binding_id) = self.ensure_binding(fact)
                    && let Some(binding) = self.state.bindings.get_mut(&binding_id)
                    && binding.method.is_none()
                {
                    binding.method = Some(proof.method);
                    binding.executable_identity.clone_from(&proof.executable_identity);
                    binding.window_hint = proof.window_hint;
                    binding.tab_hint = proof.tab_hint;
                    binding.proof = proof.evidence.clone();
                    binding.evidence_observation = Some(fact.observation_id.clone());
                    binding.proof_point.clone_from(&fact.causal);
                    binding.recorded_cursor = Some(cursor);
                }
            }
            FactPayload::SurfaceBindingUnproven { reason } => {
                if let Some(execution) = refs.execution_id.as_ref().and_then(|id| self.state.executions.get_mut(id)) {
                    execution.surface_status = Some(reason.clone());
                }
            }
            FactPayload::SurfaceBindingInvalidated { reason } => {
                if let Some(binding_id) = self.ensure_binding(fact)
                    && let Some(binding) = self.state.bindings.get_mut(&binding_id)
                {
                    binding.invalidations.insert(reason.clone());
                }
            }
            FactPayload::ProviderSnapshotObserved {
                present,
                row,
                interval,
            } => {
                if let Some(session) = refs.session_id.as_ref().and_then(|id| self.state.sessions.get_mut(id)) {
                    let newer = match (&session.inventory, &fact.causal) {
                        (Some(old), Some(point)) => old
                            .point
                            .as_ref()
                            .is_none_or(|old| compare(old, point) != CausalOrder::After),
                        _ => true,
                    };
                    if newer {
                        session.inventory = Some(InventoryObservation {
                            present: *present,
                            row: row.clone(),
                            interval: *interval,
                            point: fact.causal.clone(),
                        });
                    }
                }
            }
            FactPayload::ObservationGapDetected { domain, detail } => {
                let key = keys::coverage(&self.entry.source_id, &self.entry.source_epoch);
                snap(&self.state.coverage, &mut self.before.coverage, &key);
                let entry = self.entry;
                let record = self
                    .state
                    .coverage
                    .entry(key.clone())
                    .or_insert_with(|| SourceCoverage {
                        key,
                        source_id: entry.source_id.clone(),
                        source_epoch: entry.source_epoch.clone(),
                        meaning: threadspace_contracts::canonical::envelope::SequenceMeaning::ObserverCapture,
                        seen: Vec::new(),
                        gaps: Vec::new(),
                        reported_gaps: BTreeSet::new(),
                        revision: cursor,
                    });
                record.reported_gaps.insert(format!("{domain}: {detail}"));
            }
            FactPayload::RouteResultRecorded {
                request_id,
                surface_result,
                session_verification,
                input_readiness,
                reason_code,
                focus_performed,
            } => {
                if let Some(session) = refs.session_id.as_ref().and_then(|id| self.state.sessions.get_mut(id)) {
                    session.last_route = Some(RouteRecord {
                        request_id: request_id.clone(),
                        surface_result: *surface_result,
                        session_verification: *session_verification,
                        input_readiness: *input_readiness,
                        reason_code: reason_code.clone(),
                        focus_performed: *focus_performed,
                    });
                }
            }
            FactPayload::AttentionAcknowledged { command_id, at_ms } => {
                if let Some(id) = &refs.attention_id {
                    if !self.wait_decision(id, command_id, OwnerAction::Acknowledge, *at_ms) {
                        self.touch_attention(id);
                        if let Some(item) = self.state.attention.get_mut(id) {
                            item.acknowledgements.insert(command_id.clone());
                            item.acknowledged_at_ms.get_or_insert(*at_ms);
                        }
                    }
                    self.command(command_id, id, OwnerActionKind::Acknowledge);
                }
            }
            FactPayload::AttentionResolved {
                command_id,
                at_ms,
                reason,
            } => {
                if let Some(id) = &refs.attention_id {
                    let action = OwnerAction::Resolve { reason: reason.clone() };
                    if !self.wait_decision(id, command_id, action, *at_ms) {
                        self.touch_attention(id);
                        if let Some(item) = self.state.attention.get_mut(id) {
                            item.resolutions.insert(ResolutionCause {
                                kind: ResolutionKind::Owner,
                                detail: reason.clone(),
                            });
                            item.resolved_at_ms.get_or_insert(*at_ms);
                        }
                    }
                    self.command(command_id, id, OwnerActionKind::Resolve);
                }
            }
            FactPayload::AttentionSnoozed {
                command_id,
                at_ms,
                until_ms,
            } => {
                if let Some(id) = &refs.attention_id {
                    let action = OwnerAction::Snooze { until_ms: *until_ms };
                    if !self.wait_decision(id, command_id, action, *at_ms) {
                        self.touch_attention(id);
                        if let Some(item) = self.state.attention.get_mut(id) {
                            item.snoozed_until_ms = Some(*until_ms);
                        }
                    }
                    self.command(command_id, id, OwnerActionKind::Snooze);
                }
            }
            FactPayload::NotificationDeliveryRecorded {
                request_id,
                state,
                detail,
            } => {
                if let Some(id) = &refs.attention_id {
                    self.touch_attention(id);
                    self.touch_outbox(request_id);
                    if let Some(item) = self.state.attention.get_mut(id) {
                        item.notification_state = state.clone();
                    }
                    let outbox_state = match state {
                        NotificationState::NotRequested => OutboxState::Suppressed,
                        NotificationState::Pending => OutboxState::Pending,
                        NotificationState::Submitted => OutboxState::Submitted,
                        NotificationState::ConfirmedPresent => OutboxState::ConfirmedPresent,
                        NotificationState::Uncertain => OutboxState::Uncertain,
                        NotificationState::Failed => OutboxState::Failed,
                    };
                    let at = self.entry.captured_wall_ms;
                    let record = self
                        .state
                        .outbox
                        .entry(request_id.clone())
                        .or_insert_with(|| OutboxRecord {
                            request_id: request_id.clone(),
                            attention_id: id.clone(),
                            state: outbox_state,
                            detail: None,
                            created_at_ms: at,
                            updated_at_ms: at,
                            created_cursor: cursor,
                            revision: cursor,
                        });
                    record.state = outbox_state;
                    record.detail = Some(detail.clone());
                    record.updated_at_ms = at;
                }
            }
        }
    }

    fn command(&mut self, command_id: &str, attention_id: &str, action: OwnerActionKind) {
        self.commands.insert(command_id.to_owned());
        self.state.commands.insert(
            command_id.to_owned(),
            CommandEffect {
                command_id: command_id.to_owned(),
                attention_id: attention_id.to_owned(),
                action,
                cursor: self.entry.cursor,
            },
        );
    }

    fn request_record(&mut self, session_id: &str, request: &str, fact: &ResolvedFact) -> String {
        let key = keys::request(session_id, request);
        self.touch_request(&key);
        if !self.state.requests.contains_key(&key) {
            if let Some(turn) = &fact.refs.turn_id {
                self.index.link_turn_request(turn, &key);
            }
            self.state.requests.insert(
                key.clone(),
                ExactRequestRecord {
                    key: key.clone(),
                    session_id: session_id.to_owned(),
                    actor_id: fact.refs.actor_id.clone(),
                    turn_id: fact.refs.turn_id.clone(),
                    native_request_id: request.to_owned(),
                    category: None,
                    positive: false,
                    resolved: false,
                    attention_id: None,
                    created_cursor: self.cursor(),
                    revision: self.cursor(),
                },
            );
        }
        self.trigger.entry(key.clone()).or_insert_with(|| fact.fact_id.clone());
        key
    }

    fn wait(
        &mut self,
        fact: &ResolvedFact,
        category: WaitCategory,
        signal: WaitSignal,
        subtype: Option<&String>,
        generation: Option<&String>,
    ) {
        let refs = &fact.refs;
        let Some(session_id) = refs.session_id.clone() else { return };
        if let Some(request) = &refs.request {
            let key = self.request_record(&session_id, request, fact);
            if let Some(record) = self.state.requests.get_mut(&key) {
                record.category.get_or_insert(category);
                match signal {
                    WaitSignal::Positive => record.positive = true,
                    WaitSignal::Cleared => record.resolved = true,
                }
            }
            return;
        }
        let key = keys::wait_scope(
            &session_id,
            refs.actor_id.as_deref(),
            refs.execution_id.as_deref(),
            refs.turn_id.as_deref(),
            wait_kind(category),
            generation.map(String::as_str),
        );
        self.touch_wait(&key);
        let cursor = self.cursor();
        if !self.state.waits.contains_key(&key) {
            if let Some(turn) = &refs.turn_id {
                self.index.link_turn_wait(turn, &key);
            }
            self.state.waits.insert(
                key.clone(),
                WaitScopeRecord {
                    key: key.clone(),
                    session_id: session_id.clone(),
                    actor_id: refs.actor_id.clone(),
                    execution_id: refs.execution_id.clone(),
                    turn_id: refs.turn_id.clone(),
                    category,
                    subtypes: BTreeSet::new(),
                    generation: generation.cloned(),
                    positives: BTreeSet::new(),
                    clears: BTreeSet::new(),
                    unordered_positives: 0,
                    unordered_clears: 0,
                    episodes: Vec::new(),
                    owner_decisions: Vec::new(),
                    created_cursor: cursor,
                    revision: cursor,
                },
            );
        }
        self.trigger.entry(key.clone()).or_insert_with(|| fact.fact_id.clone());
        if let Some(record) = self.state.waits.get_mut(&key) {
            if let Some(subtype) = subtype {
                record.subtypes.insert(subtype.clone());
            }
            match (signal, &fact.causal) {
                (WaitSignal::Positive, Some(point)) => {
                    record.positives.insert(point.clone());
                }
                (WaitSignal::Cleared, Some(point)) => {
                    record.clears.insert(point.clone());
                }
                (WaitSignal::Positive, None) => {
                    record.unordered_positives = record.unordered_positives.saturating_add(1);
                }
                (WaitSignal::Cleared, None) => {
                    record.unordered_clears = record.unordered_clears.saturating_add(1);
                }
            }
        }
    }

    // ------------------------------------------------------------ deriving

    fn derive(&mut self) {
        let cursor = self.cursor();

        // Processes reach their executions.
        let processes: Vec<String> = self.before.processes.keys().cloned().collect();
        for process in processes {
            let executions: Vec<String> = self
                .index
                .process_executions
                .get(&process)
                .map(|ids| ids.iter().cloned().collect())
                .unwrap_or_default();
            for execution in executions {
                self.touch_execution(&execution);
            }
        }

        // Execution presence: ENDED is sticky and comes only from an explicit
        // end or the provider process's exit in an embedded runtime.
        let executions: Vec<String> = self.before.executions.keys().cloned().collect();
        for id in &executions {
            self.derive_attachment(id);
            let Some(execution) = self.state.executions.get(id) else { continue };
            let exited = execution
                .process_id
                .as_ref()
                .and_then(|p| self.state.processes.get(p))
                .is_some_and(|p| p.exited)
                && execution.mode == Some(ExecutionMode::TerminalEmbedded);
            let presence = if !execution.end_reasons.is_empty() || exited {
                ExecutionPresence::Ended
            } else {
                match execution.attached {
                    Some(AttachedPresence::Live) => ExecutionPresence::Live,
                    Some(AttachedPresence::Detached) => ExecutionPresence::Detached,
                    Some(AttachedPresence::Parked) => ExecutionPresence::Parked,
                    None => ExecutionPresence::Unknown,
                }
            };
            let session = execution.session_id.clone();
            if let Some(execution) = self.state.executions.get_mut(id) {
                if presence == ExecutionPresence::Ended {
                    execution.ended_cursor.get_or_insert(cursor);
                }
                execution.presence = presence;
            }
            self.touch_session(&session);
            // Its process evidence may corroborate a host-read outcome.
            let pending: Vec<String> = self
                .index
                .session_turns
                .get(&session)
                .map(|ids| {
                    ids.iter()
                        .filter(|t| self.state.turns.get(*t).is_some_and(|t| !t.pending_outcomes.is_empty()))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            for turn in pending {
                self.touch_turn(&turn);
            }
            let bindings: Vec<String> = self
                .index
                .execution_bindings
                .get(id)
                .map(|ids| ids.iter().cloned().collect())
                .unwrap_or_default();
            for binding in bindings {
                self.touch_binding(&binding);
            }
        }

        // Binding validity: permanent once lost (never revived).
        let bindings: Vec<String> = self.before.bindings.keys().cloned().collect();
        for id in &bindings {
            let Some(binding) = self.state.bindings.get(id) else { continue };
            let execution = self.state.executions.get(&binding.execution_id);
            // In-place exec invalidates a proof when the current image is
            // determinable and is not the one the binding was proven with, or
            // once another image is observed causally after the proof: later
            // evidence without a causal point cannot undo that, and an image
            // before a fresh proof does not count against it.
            let replaced = execution
                .and_then(|e| e.process_id.as_ref())
                .and_then(|p| self.state.processes.get(p))
                .zip(binding.executable_identity.as_ref())
                .is_some_and(|(process, proven)| {
                    process.current_executable.as_ref().is_some_and(|current| current != proven)
                        || replaced_after(&process.images, proven, binding.proof_point.as_ref())
                });
            let ended = execution.is_some_and(|e| e.presence == ExecutionPresence::Ended);
            let reason = if let Some(reason) = binding.invalidations.iter().next() {
                Some(reason.clone())
            } else if ended {
                Some(
                    execution
                        .and_then(|e| e.end_reasons.iter().next().cloned())
                        .unwrap_or_else(|| "PROCESS_EXITED".to_owned()),
                )
            } else if replaced {
                Some("EXECUTABLE_REPLACED".to_owned())
            } else if binding.method.is_none() {
                Some("UNPROVEN".to_owned())
            } else {
                None
            };
            let session = binding.session_id.clone();
            let execution_id = binding.execution_id.clone();
            if let Some(binding) = self.state.bindings.get_mut(id) {
                binding.valid = reason.is_none();
                if reason.is_some() && binding.recorded_cursor.is_some() {
                    binding.invalidated_cursor.get_or_insert(cursor);
                }
                binding.invalidation_reason = reason;
            }
            self.touch_session(&session);
            self.touch_execution(&execution_id);
        }

        // Each touched input re-derives its origin and submission; its
        // session's frontiers are then re-derived from all its inputs, so a
        // late rejection withdraws what an earlier acceptance advanced.
        let inputs: Vec<String> = self.before.inputs.keys().cloned().collect();
        let mut sessions = BTreeSet::new();
        for id in &inputs {
            self.derive_input(id);
            if let Some(input) = self.state.inputs.get(id) {
                sessions.insert(input.session_id.clone());
            }
        }
        for session in &sessions {
            self.derive_frontiers(session);
        }

        // Turns whose waits, requests or frontier changed are re-derived.
        let waits: Vec<String> = self.before.waits.keys().cloned().collect();
        for key in &waits {
            self.derive_wait(key);
            if let Some(turn) = self.state.waits.get(key).and_then(|w| w.turn_id.clone()) {
                self.touch_turn(&turn);
            }
        }
        let requests: Vec<String> = self.before.requests.keys().cloned().collect();
        for key in &requests {
            self.derive_request(key);
            if let Some(turn) = self.state.requests.get(key).and_then(|r| r.turn_id.clone()) {
                self.touch_turn(&turn);
            }
        }
        let frontiers: Vec<String> = self.before.frontiers.keys().cloned().collect();
        for key in &frontiers {
            let Some(frontier) = self.state.frontiers.get(key) else { continue };
            let (session, actor) = (frontier.session_id.clone(), frontier.actor_id.clone());
            let turns: Vec<String> = self
                .index
                .session_turns
                .get(&session)
                .map(|ids| {
                    ids.iter()
                        .filter(|id| self.state.turns.get(*id).is_some_and(|t| t.actor_id == actor))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            for turn in turns {
                self.touch_turn(&turn);
            }
        }
        let turns: Vec<String> = self.before.turns.keys().cloned().collect();
        for id in &turns {
            self.derive_turn(id);
        }

        // Attention, then its outbox.
        let attention: Vec<String> = self.before.attention.keys().cloned().collect();
        for id in &attention {
            self.derive_attention_resolution(id);
        }
        let attention: Vec<String> = self.before.attention.keys().cloned().collect();
        for id in &attention {
            self.derive_outbox(id);
        }

        let sessions: Vec<String> = self.before.sessions.keys().cloned().collect();
        for id in &sessions {
            self.derive_session(id);
        }
    }

    /// Mode, presence, runtime ID and device from the attach observations no
    /// other follows. Disagreement settles conservatively and is flagged: the
    /// lowest mode, the least live presence, no runtime ID or device. With no
    /// observation the baseline's values stand.
    fn derive_attachment(&mut self, id: &str) {
        let Some(execution) = self.state.executions.get(id) else { return };
        if execution.attachments.is_empty() {
            return;
        }
        let all: Vec<&AttachObservation> = execution.attachments.iter().collect();
        let newest = latest(all.clone(), |a| a.point.as_ref());
        let modes: BTreeSet<ExecutionMode> = newest.iter().map(|a| a.mode).collect();
        let presences: BTreeSet<AttachedPresence> = newest.iter().map(|a| a.presence).collect();
        let named = |field: fn(&AttachObservation) -> bool| -> Vec<&AttachObservation> {
            latest(all.iter().copied().filter(|a| field(a)).collect(), |a| a.point.as_ref())
        };
        let (runtime, runtime_conflict) = agreed(
            named(|a| a.native_runtime_id.is_some())
                .into_iter()
                .filter_map(|a| a.native_runtime_id.clone()),
        );
        let (device, device_conflict) =
            agreed(named(|a| a.controlling_device.is_some()).into_iter().filter_map(|a| a.controlling_device));
        let mode = modes.first().copied();
        let attached = presences.iter().copied().max_by_key(|p| presence_rank(*p));
        let conflict = modes.len() > 1 || presences.len() > 1 || runtime_conflict || device_conflict;
        if let Some(execution) = self.state.executions.get_mut(id) {
            execution.mode = mode;
            execution.attached = attached;
            execution.native_runtime_id = runtime;
            execution.controlling_device = device;
            execution.attachment_conflict = conflict;
        }
        if conflict {
            self.note("ATTACHMENT_CONFLICT", id.to_owned());
        }
    }

    /// Origin, original submission point and active turn from the
    /// submission reports no other follows. Disagreeing origins are
    /// UNCLASSIFIED, which never qualifies a follow-up; disagreeing points or
    /// turns are unknown.
    fn derive_input(&mut self, id: &str) {
        let Some(input) = self.state.inputs.get(id) else { return };
        if input.submissions.is_empty() {
            return;
        }
        let newest = latest(input.submissions.iter().collect(), |s| s.point.as_ref());
        let origins: BTreeSet<InputOrigin> = newest.iter().map(|s| s.origin).collect();
        let origin = if origins.len() == 1 {
            origins.first().copied()
        } else {
            Some(InputOrigin::Unclassified)
        };
        let (submission, _) = agreed(newest.iter().filter_map(|s| s.point.clone()));
        let (active, _) = agreed(newest.iter().filter_map(|s| s.active_turn_id.clone()));
        if let Some(input) = self.state.inputs.get_mut(id) {
            input.origin = origin;
            input.submission = submission;
            input.active_turn_id = active;
        }
    }

    /// A session's human-follow-up frontiers, re-derived from every input it
    /// holds: one per (actor, order domain) of the inputs with verified
    /// accepted human provenance and no rejection.
    fn derive_frontiers(&mut self, session_id: &str) {
        let capable = self
            .state
            .sessions
            .get(session_id)
            .and_then(|s| self.state.namespaces.get(&s.namespace_id))
            .is_some_and(|n| profiles::capabilities(&n.provider, &n.profile_ref).accepted_input_provenance);
        let mut derived: BTreeMap<String, HumanFrontier> = BTreeMap::new();
        let inputs = self.index.session_inputs.get(session_id).cloned().unwrap_or_default();
        for input in inputs.iter().filter_map(|id| self.state.inputs.get(id)).filter(|_| capable) {
            let qualified = input.origin.is_some_and(|o| o.is_human())
                && input.acceptances.iter().any(|proof| proof.qualified())
                && input.rejections.is_empty();
            let Some(point) = input.submission.as_ref().filter(|_| qualified) else { continue };
            let key = keys::frontier(
                session_id,
                &input.actor_id,
                &point.source_id,
                &point.source_epoch,
                &point.order_domain,
            );
            let frontier = derived.entry(key.clone()).or_insert_with(|| HumanFrontier {
                key,
                session_id: session_id.to_owned(),
                actor_id: input.actor_id.clone(),
                source_id: point.source_id.clone(),
                source_epoch: point.source_epoch.clone(),
                order_domain: point.order_domain.clone(),
                max_sequence: None,
                predecessor_keys: BTreeSet::new(),
            });
            if let Some(sequence) = point.sequence.as_deref().and_then(parse_cursor) {
                let current = frontier.max_sequence.as_deref().and_then(parse_cursor);
                if current.is_none_or(|c| sequence > c) {
                    frontier.max_sequence = Some(format_cursor(sequence));
                }
            }
            frontier
                .predecessor_keys
                .extend(point.native_predecessor_keys.iter().cloned());
        }
        let existing = self.index.session_frontiers.get(session_id).cloned().unwrap_or_default();
        let mut changed_actors = BTreeSet::new();
        for key in existing.iter().chain(derived.keys()).cloned().collect::<BTreeSet<_>>() {
            let new = derived.remove(&key);
            if self.state.frontiers.get(&key) == new.as_ref() {
                continue;
            }
            self.touch_frontier(&key);
            if let Some(actor) = new
                .as_ref()
                .or_else(|| self.state.frontiers.get(&key))
                .map(|f| f.actor_id.clone())
            {
                changed_actors.insert(actor);
            }
            match new {
                Some(frontier) => {
                    self.index.link_frontier(session_id, &key);
                    self.state.frontiers.insert(key, frontier);
                }
                None => {
                    self.index.unlink_frontier(session_id, &key);
                    self.state.frontiers.remove(&key);
                }
            }
        }
        let turns: Vec<String> = self
            .index
            .session_turns
            .get(session_id)
            .map(|ids| {
                ids.iter()
                    .filter(|id| self.state.turns.get(*id).is_some_and(|t| changed_actors.contains(&t.actor_id)))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        for turn in turns {
            self.touch_turn(&turn);
        }
    }

    /// True when kernel/inventory evidence shows `process_id` running one of
    /// the session's executions: an execution of that process attached by
    /// anything other than a host-read report.
    fn corroborated(&self, session_id: &str, process_id: &str) -> bool {
        self.index.session_executions.get(session_id).is_some_and(|ids| {
            ids.iter().filter_map(|id| self.state.executions.get(id)).any(|execution| {
                execution.process_id.as_deref() == Some(process_id)
                    && execution
                        .attachments
                        .iter()
                        .any(|a| a.provenance != EvidenceClass::HostRead)
            })
        })
    }

    /// The link from the reports no other follows (disagreement settles on
    /// the most degraded and is flagged), and the observer's evidence tier:
    /// native when a qualified report carries engine-stamped identity,
    /// restored when a qualified host-read report's provider process is
    /// corroborated, lower tier otherwise. With no report the baseline's
    /// link stands.
    fn derive_link(&mut self, id: &str) {
        let Some(session) = self.state.sessions.get(id) else { return };
        if session.links.is_empty() {
            return;
        }
        let newest = latest(session.links.iter().collect(), |l| l.point.as_ref());
        let states: BTreeSet<ObservationState> = newest.iter().map(|l| l.link.clone()).collect();
        let link = states.iter().max_by_key(|s| link_rank(s)).cloned();
        let conflict = states.len() > 1;
        let (version, _) = agreed(newest.iter().filter_map(|l| l.version.clone()));
        let tier = if newest.iter().any(|l| l.qualified && l.provenance != EvidenceClass::HostRead) {
            ObserverTier::Native
        } else if newest.iter().any(|l| {
            l.qualified && l.process_id.as_deref().is_some_and(|p| self.corroborated(id, p))
        }) {
            ObserverTier::Restored
        } else {
            ObserverTier::LowerTier
        };
        if let Some(session) = self.state.sessions.get_mut(id) {
            session.link = link;
            session.link_conflict = conflict;
            session.observer_tier = Some(tier);
            session.observer_version = version;
        }
    }

    /// Partitions a wait scope's positives into episodes by the comparable
    /// clears before them, and settles each episode from its evidence.
    fn derive_wait(&mut self, key: &str) {
        let Some(scope) = self.state.waits.get(key) else { return };
        let previous: Vec<(u32, Option<String>)> =
            scope.episodes.iter().map(|e| (e.index, e.attention_id.clone())).collect();
        let episodes = wait::partition(scope);
        let decisions = scope.owner_decisions.clone();
        let session_id = scope.session_id.clone();
        let actor_id = scope.actor_id.clone();
        let turn_id = scope.turn_id.clone();
        let category = scope.category;
        let trigger = self.trigger.get(key).cloned();
        let mut derived = Vec::new();
        for (index, episode) in episodes {
            let episode_id = derived_id(&format!("{key}#{index}"));
            let (attention_category, priority) = wait_attention(category);
            let scope_value = AttentionScope::SessionWaitCategory {
                episode_id: episode_id.clone(),
                wait_kind: wait_kind(category).to_owned(),
            };
            let attention_id = self.attention_item(
                &session_id,
                actor_id.as_ref(),
                turn_id.as_ref(),
                scope_value,
                attention_category,
                priority,
                trigger.clone(),
                None,
            );
            let ended = !episode.open && !episode.uncertain;
            self.set_native_resolution(
                &attention_id,
                ResolutionKind::WaitEnded,
                ended,
                "native wait cleared",
            );
            self.set_native_resolution(&attention_id, ResolutionKind::WaitEnded, false, WAIT_SUPERSEDED);
            self.apply_owner_decisions(&attention_id, &wait::owner_state(&decisions, &episode));
            derived.push(WaitEpisode {
                index,
                episode_id,
                active: episode.open || episode.uncertain,
                uncertain: episode.uncertain,
                attention_id: Some(attention_id),
            });
        }
        // A late, earlier clear moves an episode's positives into a later
        // episode, which now carries the wait and the owner decisions made
        // on that evidence; the emptied episode's item stops claiming an
        // open wait (it never existed had the clear come first, so semantic
        // equality leaves it out).
        for (index, attention_id) in previous {
            if let Some(id) = attention_id.filter(|_| !derived.iter().any(|e| e.index == index)) {
                self.set_native_resolution(&id, ResolutionKind::WaitEnded, true, WAIT_SUPERSEDED);
                self.apply_owner_decisions(&id, &wait::OwnerState::default());
            }
        }
        if let Some(scope) = self.state.waits.get_mut(key) {
            scope.episodes = derived;
        }
    }

    /// A wait item's owner state is derived from the scope's owner decisions
    /// that apply to its episode's evidence (D-0007 §4).
    fn apply_owner_decisions(&mut self, id: &str, owner: &wait::OwnerState<'_>) {
        self.touch_attention(id);
        let Some(item) = self.state.attention.get_mut(id) else { return };
        item.acknowledgements.clear();
        item.acknowledged_at_ms = None;
        item.resolutions.retain(|cause| cause.kind != ResolutionKind::Owner);
        item.snoozed_until_ms = None;
        if let Some(acknowledge) = &owner.acknowledge {
            item.acknowledgements
                .extend(acknowledge.decisions.iter().map(|d| d.command_id.clone()));
            item.acknowledged_at_ms = Some(acknowledge.at_ms);
        }
        for decision in owner.resolve.iter().flat_map(|resolve| &resolve.decisions) {
            if let OwnerAction::Resolve { reason } = &decision.action {
                item.resolutions.insert(ResolutionCause {
                    kind: ResolutionKind::Owner,
                    detail: reason.clone(),
                });
            }
        }
        item.snoozed_until_ms = owner.snooze.as_ref().and_then(|snooze| snooze.until_ms);
    }

    /// Keeps an owner command on a wait item with the evidence of the episode
    /// it was made on; false for any other item. An item whose episode a
    /// late clear already emptied records a decision covering nothing.
    fn wait_decision(&mut self, id: &str, command_id: &str, action: OwnerAction, at_ms: i64) -> bool {
        let Some(item) = self.state.attention.get(id) else { return false };
        let AttentionScope::SessionWaitCategory { episode_id, .. } = &item.scope else { return false };
        let session_id = item.session_id.clone();
        let found = self.state.waits.iter().find_map(|(key, scope)| {
            if scope.session_id != session_id {
                return None;
            }
            (0..=u32::try_from(scope.clears.len()).unwrap_or(u32::MAX))
                .find(|index| derived_id(&format!("{key}#{index}")) == *episode_id)
                .map(|index| (key.clone(), index))
        });
        let Some((key, index)) = found else { return false };
        self.touch_wait(&key);
        let Some(scope) = self.state.waits.get_mut(&key) else { return false };
        let episode = wait::partition(scope).remove(&index).unwrap_or_default();
        if !scope.owner_decisions.iter().any(|d| d.command_id == command_id) {
            scope.owner_decisions.push(WaitOwnerDecision {
                command_id: command_id.to_owned(),
                action,
                at_ms,
                positives: episode.positives,
                unordered: episode.unordered,
            });
            scope.owner_decisions.sort_by(|a, b| a.command_id.cmp(&b.command_id));
        }
        true
    }

    fn derive_request(&mut self, key: &str) {
        let Some(request) = self.state.requests.get(key) else { return };
        if !request.positive {
            return;
        }
        let (category, priority) = wait_attention(request.category.unwrap_or(WaitCategory::Approval));
        let scope = AttentionScope::ExactRequest {
            native_request_id: request.native_request_id.clone(),
        };
        let session_id = request.session_id.clone();
        let actor_id = request.actor_id.clone();
        let turn_id = request.turn_id.clone();
        let resolved = request.resolved;
        let trigger = self.trigger.get(key).cloned();
        let attention_id = self.attention_item(
            &session_id,
            actor_id.as_ref(),
            turn_id.as_ref(),
            scope,
            category,
            priority,
            trigger,
            None,
        );
        self.set_native_resolution(
            &attention_id,
            ResolutionKind::RequestResolved,
            resolved,
            "native request resolved",
        );
        if let Some(request) = self.state.requests.get_mut(key) {
            request.attention_id = Some(attention_id);
        }
    }

    /// Applies each host-read outcome whose provider process is now
    /// corroborated. Corroboration only grows, so the result does not depend
    /// on whether the outcome or the process evidence arrived first.
    fn promote_pending(&mut self, id: &str) {
        let Some(turn) = self.state.turns.get(id) else { return };
        let promoted: Vec<PendingOutcome> = turn
            .pending_outcomes
            .iter()
            .filter(|p| p.process_id.as_deref().is_some_and(|process| self.corroborated(&turn.session_id, process)))
            .cloned()
            .collect();
        if let Some(turn) = self.state.turns.get_mut(id) {
            for pending in promoted {
                turn.outcomes.insert(pending.outcome);
                turn.outcome_reasons.extend(pending.reason);
                turn.output_points.extend(pending.point);
            }
        }
    }

    fn derive_turn(&mut self, id: &str) {
        self.promote_pending(id);
        let Some(turn) = self.state.turns.get(id) else { return };
        let waiting = self.index.turn_waits.get(id).is_some_and(|keys| {
            keys.iter()
                .filter_map(|key| self.state.waits.get(key))
                .any(|wait| wait.episodes.iter().any(|e| e.active))
        }) || self.index.turn_requests.get(id).is_some_and(|keys| {
            keys.iter()
                .filter_map(|key| self.state.requests.get(key))
                .any(|r| r.positive && !r.resolved)
        });
        let distinct: BTreeSet<TurnOutcome> = turn.outcomes.clone();
        let conflict = distinct.len() > 1;
        // A terminal outcome closes the turn: late activity, steps or a
        // delayed start never make it working again (INV-08).
        let state = if let Some(worst) = distinct.iter().copied().max_by_key(|o| severity(*o)) {
            outcome_state(worst)
        } else if waiting {
            TurnState::Waiting
        } else if turn.started || turn.stepped || turn.activity_seen {
            TurnState::Working
        } else if !turn.queued_inputs.is_empty() {
            TurnState::Queued
        } else {
            TurnState::Unknown
        };
        let output = turn.owner_facing
            && (turn.output_ready
                || distinct
                    .iter()
                    .any(|o| matches!(o, TurnOutcome::Completed | TurnOutcome::Failed | TurnOutcome::Refused)));
        let failed = distinct
            .iter()
            .any(|o| matches!(o, TurnOutcome::Failed | TurnOutcome::Refused));
        let session_id = turn.session_id.clone();
        let actor_id = turn.actor_id.clone();
        let summary = turn.summary.clone();
        let points = turn.output_points.clone();
        if let Some(turn) = self.state.turns.get_mut(id) {
            turn.state = state;
            turn.outcome_conflict = conflict;
        }
        if conflict {
            self.note("TURN_OUTCOME_CONFLICT", id.to_owned());
        }
        self.touch_session(&session_id);
        if !output {
            return;
        }
        let (category, priority) = if failed {
            (AttentionCategory::Error, PRIORITY_ERROR)
        } else {
            (AttentionCategory::TurnComplete, PRIORITY_OUTPUT)
        };
        let trigger = self.trigger.get(id).cloned();
        let attention_id = self.attention_item(
            &session_id,
            Some(&actor_id),
            Some(&id.to_owned()),
            AttentionScope::TurnOutput {
                turn_id: id.to_owned(),
            },
            category.clone(),
            priority,
            trigger,
            summary,
        );
        // Only an eligible previous output resolves on follow-up; never an
        // error, approval, blocker or owner decision (SPEC §7.3 (5)).
        let followed = category == AttentionCategory::TurnComplete
            && self.followed_by_human(&session_id, &actor_id, &points);
        self.set_native_resolution(
            &attention_id,
            ResolutionKind::HumanFollowup,
            followed,
            "accepted human follow-up",
        );
    }

    fn followed_by_human(&self, session_id: &str, actor_id: &str, points: &BTreeSet<CausalPoint>) -> bool {
        let Some(namespace) = self
            .state
            .sessions
            .get(session_id)
            .and_then(|s| self.state.namespaces.get(&s.namespace_id))
        else {
            return false;
        };
        if !profiles::capabilities(&namespace.provider, &namespace.profile_ref)
            .automatic_human_followup_resolution
        {
            return false;
        }
        let Some(frontiers) = self.index.session_frontiers.get(session_id) else {
            return false;
        };
        frontiers
            .iter()
            .filter_map(|key| self.state.frontiers.get(key))
            .filter(|f| f.actor_id == actor_id)
            .any(|frontier| {
                points.iter().any(|point| {
                    let ordered = point.source_id == frontier.source_id
                        && point.source_epoch == frontier.source_epoch
                        && point.order_domain == frontier.order_domain
                        && matches!(
                            (
                                point.sequence.as_deref().and_then(parse_cursor),
                                frontier.max_sequence.as_deref().and_then(parse_cursor),
                            ),
                            (Some(output), Some(input)) if output < input
                        );
                    let named = point
                        .native_key
                        .as_ref()
                        .is_some_and(|key| frontier.predecessor_keys.contains(key));
                    ordered || named
                })
            })
    }

    /// Finds or creates the one item of a scope (SPEC §7.1), updating its
    /// category only toward greater severity.
    #[allow(clippy::too_many_arguments)]
    fn attention_item(
        &mut self,
        session_id: &str,
        actor_id: Option<&String>,
        turn_id: Option<&String>,
        scope: AttentionScope,
        category: AttentionCategory,
        priority: u8,
        trigger: Option<String>,
        summary: Option<String>,
    ) -> String {
        let scope_key = keys::attention_scope(session_id, scope.scope_kind(), scope.scope_key());
        let id = self
            .state
            .attention_by_scope
            .get(&scope_key)
            .cloned()
            .unwrap_or_else(|| derived_id(&scope_key));
        self.touch_attention(&id);
        let cursor = self.cursor();
        match self.state.attention.get_mut(&id) {
            Some(item) => {
                if priority > item.priority {
                    item.category = category;
                    item.priority = priority;
                }
                if item.summary.is_none() && summary.is_some() {
                    item.summary = summary;
                    item.summary_authority = SummaryAuthority::NativeMetadata;
                }
            }
            None => {
                let authority = if summary.is_some() {
                    SummaryAuthority::NativeMetadata
                } else {
                    SummaryAuthority::None
                };
                self.state.attention_by_scope.insert(scope_key, id.clone());
                self.state.attention.insert(
                    id.clone(),
                    AttentionRecord {
                        id: id.clone(),
                        session_id: session_id.to_owned(),
                        actor_id: actor_id.cloned(),
                        turn_id: turn_id.cloned(),
                        category,
                        scope,
                        priority,
                        summary,
                        summary_authority: authority,
                        created_by_fact: trigger.unwrap_or_else(|| {
                            self.entry
                                .facts
                                .first()
                                .map(|f| f.fact_id.clone())
                                .unwrap_or_default()
                        }),
                        created_by_observation: self.entry.observation_id.clone(),
                        created_at_ms: self.entry.captured_wall_ms,
                        acknowledgements: BTreeSet::new(),
                        acknowledged_at_ms: None,
                        resolutions: BTreeSet::new(),
                        resolved_at_ms: None,
                        resolution_reason: None,
                        snoozed_until_ms: None,
                        notification_state: NotificationState::NotRequested,
                        created_cursor: cursor,
                        revision: cursor,
                    },
                );
            }
        }
        id
    }

    /// A native resolution cause is re-derived from current evidence each
    /// time (so incomparable late evidence can withhold it); an owner
    /// resolution is never removed.
    fn set_native_resolution(&mut self, id: &str, kind: ResolutionKind, on: bool, detail: &str) {
        self.touch_attention(id);
        if let Some(item) = self.state.attention.get_mut(id) {
            let cause = ResolutionCause {
                kind,
                detail: detail.to_owned(),
            };
            if on {
                item.resolutions.insert(cause);
            } else {
                item.resolutions.remove(&cause);
            }
        }
    }

    fn derive_attention_resolution(&mut self, id: &str) {
        let at = self.entry.captured_wall_ms;
        if let Some(item) = self.state.attention.get_mut(id) {
            if item.resolutions.is_empty() {
                item.resolved_at_ms = None;
                item.resolution_reason = None;
            } else {
                item.resolved_at_ms.get_or_insert(at);
                // An owner's reason is shown first; otherwise the native cause.
                item.resolution_reason = item
                    .resolutions
                    .iter()
                    .find(|c| c.kind == ResolutionKind::Owner)
                    .or_else(|| item.resolutions.iter().find(|c| is_native_resolution(c.kind)))
                    .map(|c| c.detail.clone());
            }
            let session = item.session_id.clone();
            self.touch_session(&session);
        }
    }

    /// Intent in the same transaction as its attention (SPEC §7.6); an item
    /// that stops being eligible before submission suppresses its intent.
    fn derive_outbox(&mut self, id: &str) {
        let Some(item) = self.state.attention.get(id) else { return };
        let eligible = !item.resolved() && !item.acknowledged();
        let recorded: Vec<String> = self
            .index
            .attention_outbox
            .get(id)
            .map(|requests| requests.iter().cloned().collect())
            .unwrap_or_default();
        let at = self.entry.captured_wall_ms;
        let cursor = self.cursor();
        if !eligible {
            for request_id in &recorded {
                if !matches!(
                    self.state.outbox.get(request_id).map(|o| o.state),
                    Some(OutboxState::Pending | OutboxState::Held)
                ) {
                    continue;
                }
                self.touch_outbox(request_id);
                if let Some(outbox) = self.state.outbox.get_mut(request_id) {
                    outbox.state = OutboxState::Suppressed;
                    outbox.detail = Some(SUPPRESSED_BEFORE_SUBMISSION.to_owned());
                    outbox.updated_at_ms = at;
                }
                if let Some(item) = self.state.attention.get_mut(id) {
                    item.notification_state = NotificationState::NotRequested;
                }
            }
        } else if self.entry.delivery != Delivery::Bootstrap {
            let state = if self.entry.delivery == Delivery::Live {
                OutboxState::Pending
            } else {
                OutboxState::Held
            };
            // An intent this reducer suppressed before submission is current
            // eligibility, not history: it re-arms once its item is eligible
            // again. Submitted intents and migrated M0 ones are history.
            let suppressed: Vec<String> = recorded
                .iter()
                .filter(|request_id| {
                    self.state.outbox.get(*request_id).is_some_and(|o| {
                        o.state == OutboxState::Suppressed
                            && o.detail.as_deref() == Some(SUPPRESSED_BEFORE_SUBMISSION)
                    })
                })
                .cloned()
                .collect();
            for request_id in suppressed {
                self.touch_outbox(&request_id);
                if let Some(outbox) = self.state.outbox.get_mut(&request_id) {
                    outbox.state = state;
                    outbox.detail = None;
                    outbox.updated_at_ms = at;
                }
                if let Some(item) = self.state.attention.get_mut(id) {
                    item.notification_state = NotificationState::Pending;
                }
                if state == OutboxState::Pending {
                    self.new_outbox.push(request_id);
                }
            }
            if !recorded.is_empty() {
                return;
            }
            let request_id = derived_id(&format!("outbox|{id}"));
            self.touch_outbox(&request_id);
            self.index.link_outbox(id, &request_id);
            self.state.outbox.insert(
                request_id.clone(),
                OutboxRecord {
                    request_id: request_id.clone(),
                    attention_id: id.to_owned(),
                    state,
                    detail: None,
                    created_at_ms: at,
                    updated_at_ms: at,
                    created_cursor: cursor,
                    revision: cursor,
                },
            );
            if let Some(item) = self.state.attention.get_mut(id) {
                item.notification_state = NotificationState::Pending;
            }
            if state == OutboxState::Pending {
                self.new_outbox.push(request_id);
            }
        }
    }

    fn derive_session(&mut self, id: &str) {
        self.derive_link(id);
        let Some(session) = self.state.sessions.get(id) else { return };
        let executions: Vec<&ExecutionRecord> = self
            .index
            .session_executions
            .get(id)
            .map(|ids| ids.iter().filter_map(|e| self.state.executions.get(e)).collect())
            .unwrap_or_default();
        let presence = if executions.iter().any(|e| e.presence == ExecutionPresence::Live) {
            ExecutionPresence::Live
        } else if executions.iter().any(|e| e.presence == ExecutionPresence::Detached) {
            ExecutionPresence::Detached
        } else if executions.iter().any(|e| e.presence == ExecutionPresence::Parked) {
            ExecutionPresence::Parked
        } else if !executions.is_empty()
            && executions.iter().all(|e| e.presence == ExecutionPresence::Ended)
        {
            ExecutionPresence::Ended
        } else {
            ExecutionPresence::Unknown
        };
        let observation = if session.fixture {
            ObservationState::Unknown
        } else if let Some(link) = &session.link {
            link.clone()
        } else {
            match &session.inventory {
                Some(inventory) if inventory.present => ObservationState::Current,
                Some(_) => ObservationState::Stale,
                None => ObservationState::Unknown,
            }
        };
        let turns: Vec<&TurnRecord> = self
            .index
            .session_turns
            .get(id)
            .map(|ids| ids.iter().filter_map(|t| self.state.turns.get(t)).collect())
            .unwrap_or_default();
        let turn_state = if turns.iter().any(|t| t.state == TurnState::Waiting) {
            TurnState::Waiting
        } else if turns.iter().any(|t| t.state == TurnState::Working) {
            TurnState::Working
        } else if turns.iter().any(|t| t.state == TurnState::Queued) {
            TurnState::Queued
        } else {
            latest_settled_state(&turns)
        };
        if let Some(session) = self.state.sessions.get_mut(id) {
            session.execution_presence = presence;
            session.observation = observation;
            session.turn_state = turn_state;
        }
    }

    // ------------------------------------------------------------ settling

    fn finish(self) -> ReduceOutput {
        let cursor = self.entry.cursor;
        let mut output = ReduceOutput {
            new_outbox: self.new_outbox,
            notes: self.notes,
            ..ReduceOutput::default()
        };
        let changed = &mut output.changed;
        let state = self.state;
        let before = &self.before;
        settle_plain(&state.namespaces, &before.namespaces, &mut changed.namespaces);
        settle(&mut state.sessions, &before.sessions, &mut changed.sessions, cursor, |r| &mut r.revision, false);
        settle(&mut state.actors, &before.actors, &mut changed.actors, cursor, |r| &mut r.revision, false);
        settle(&mut state.processes, &before.processes, &mut changed.processes, cursor, |r| &mut r.revision, false);
        settle(&mut state.executions, &before.executions, &mut changed.executions, cursor, |r| &mut r.revision, false);
        settle(&mut state.turns, &before.turns, &mut changed.turns, cursor, |r| &mut r.revision, false);
        settle(&mut state.inputs, &before.inputs, &mut changed.inputs, cursor, |r| &mut r.revision, false);
        settle(&mut state.activities, &before.activities, &mut changed.activities, cursor, |r| &mut r.revision, false);
        settle(&mut state.surfaces, &before.surfaces, &mut changed.surfaces, cursor, |r| &mut r.revision, false);
        settle(&mut state.bindings, &before.bindings, &mut changed.bindings, cursor, |r| &mut r.revision, true);
        settle(&mut state.waits, &before.waits, &mut changed.waits, cursor, |r| &mut r.revision, false);
        settle(&mut state.requests, &before.requests, &mut changed.requests, cursor, |r| &mut r.revision, false);
        settle(&mut state.attention, &before.attention, &mut changed.attention, cursor, |r| &mut r.revision, false);
        settle(&mut state.outbox, &before.outbox, &mut changed.outbox, cursor, |r| &mut r.revision, false);
        settle(&mut state.coverage, &before.coverage, &mut changed.coverage, cursor, |r| &mut r.revision, false);
        settle_plain(&state.frontiers, &before.frontiers, &mut changed.frontiers);
        // A session's view shows its activations, bindings and turns: its
        // revision advances whenever one of them changed (SPEC §18.4 row
        // revisions), even if the session record itself did not.
        let parents: Vec<String> = changed
            .executions
            .iter()
            .filter_map(|id| state.executions.get(id).map(|e| e.session_id.clone()))
            .chain(changed.bindings.iter().filter_map(|id| state.bindings.get(id).map(|b| b.session_id.clone())))
            .chain(changed.turns.iter().filter_map(|id| state.turns.get(id).map(|t| t.session_id.clone())))
            .collect();
        for id in parents {
            if let Some(session) = state.sessions.get_mut(&id) {
                session.revision = cursor;
                changed.sessions.insert(id);
            }
        }
        changed.relations = self.relations_changed;
        changed.commands = self.commands;
        output
    }
}
