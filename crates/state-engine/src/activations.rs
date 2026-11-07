//! Native activation evidence and its derived execution equivalence classes.
//! No receipt order, wall clock, or independent source counter orders a start.
use crate::causal::compare;
use std::collections::{BTreeMap, BTreeSet};
use threadspace_contracts::canonical::causal::CausalOrder;
use threadspace_contracts::canonical::fact::{
    ActivationEvidenceKind as K, EvidenceClass, FactPayload, ResolvedFact,
};
use threadspace_contracts::canonical::records::CanonicalState;

fn generation(f: &ResolvedFact) -> Option<&str> {
    f.native.activation.as_ref()?.generation.as_deref()
}
fn kind(f: &ResolvedFact) -> Option<K> {
    Some(f.native.activation.as_ref()?.kind)
}
fn source(f: &ResolvedFact) -> Option<&str> {
    f.native.activation.as_ref()?.start_source.as_deref()
}
fn same_runtime(a: &ResolvedFact, b: &ResolvedFact) -> bool {
    a.refs.process_id == b.refs.process_id && a.refs.namespace_id == b.refs.namespace_id
}
fn before(a: &ResolvedFact, b: &ResolvedFact) -> bool {
    if let (Some(a_point), Some(b_point)) = (&a.causal, &b.causal) {
        let mut a_point = a_point.clone();
        let mut b_point = b_point.clone();
        if let Some(entry) = a
            .native
            .activation
            .as_ref()
            .and_then(|a| a.entry_sequence.clone())
        {
            a_point.sequence = Some(entry);
        }
        if let Some(entry) = b
            .native
            .activation
            .as_ref()
            .and_then(|a| a.entry_sequence.clone())
        {
            b_point.sequence = Some(entry);
        }
        if compare(&a_point, &b_point) == CausalOrder::Before {
            return true;
        }
    }
    // startup opens the process's initial session; clear/resume opens another.
    // This is provider source semantics, not an order invented by a helper.
    same_runtime(a, b)
        && kind(a) == Some(K::Start)
        && kind(b) == Some(K::Start)
        && source(a) == Some("startup")
        && matches!(source(b), Some("clear" | "resume"))
}
fn signature(a: &ResolvedFact, b: &ResolvedFact) -> bool {
    same_runtime(a, b) && a.refs.session_id == b.refs.session_id && source(a) == source(b)
}
struct Group<'a> {
    starts: Vec<&'a ResolvedFact>,
    members: BTreeSet<String>,
}
impl Group<'_> {
    fn first(&self) -> &ResolvedFact {
        self.starts[0]
    }
    fn representative(&self, state: &CanonicalState) -> Option<String> {
        self.starts
            .iter()
            .filter_map(|f| f.refs.execution_id.as_ref())
            .min_by_key(|id| {
                state
                    .activation_candidates
                    .get(*id)
                    .map(|e| &e.activation_ref)
            })
            .cloned()
    }
}

pub fn derive(
    state: &mut CanonicalState,
    fact_ids: &BTreeSet<String>,
    turn_ids: &BTreeSet<String>,
    execution_ids: &BTreeSet<String>,
    binding_ids: &BTreeSet<String>,
    sessions: &BTreeSet<String>,
    structural: bool,
) {
    let facts: Vec<_> = fact_ids.iter().chain(turn_ids).filter_map(|id|state.activation_facts.get(id)).cloned().collect();
    let starts: Vec<_> = facts.iter().filter(|f| kind(f) == Some(K::Start)).collect();
    let mut groups: Vec<Group<'_>> = Vec::new();
    // An observer reference is frozen per callback opening, not callback result.
    // Entry/result duplicates share it; independently proven openings do not.
    for start in starts
        .iter()
        .copied()
        .filter(|f| generation(f).is_some() && source(f).is_some())
    {
        let existing = groups.iter().position(|g| {
            g.starts.iter().any(|old| {
                same_runtime(old, start)
                    && old.refs.session_id == start.refs.session_id
                    && old.native.execution == start.native.execution
            })
        });
        let i = existing.unwrap_or_else(|| {
            groups.push(Group {
                starts: vec![],
                members: BTreeSet::new(),
            });
            groups.len() - 1
        });
        if let Some(id) = &start.refs.execution_id {
            groups[i].members.insert(id.clone());
        }
        groups[i].starts.push(start);
    }
    // Engine session.start can corroborate classic.SessionStart in the same
    // frozen generation. It is not another opening merely because its own
    // callback has a different observation ID.
    for start in starts
        .iter()
        .copied()
        .filter(|f| generation(f).is_some() && source(f).is_none())
    {
        let matches: Vec<_> = groups
            .iter()
            .enumerate()
            .filter(|(_, g)| {
                same_runtime(g.first(), start)
                    && g.first().refs.session_id == start.refs.session_id
                    && generation(g.first()) == generation(start)
            })
            .map(|(i, _)| i)
            .collect();
        if matches.len() == 1 {
            if let Some(id) = &start.refs.execution_id {
                groups[matches[0]].members.insert(id.clone());
            }
        } else if matches.is_empty() {
            groups.push(Group {
                starts: vec![start],
                members: start.refs.execution_id.clone().into_iter().collect(),
            });
        }
    }
    // Hooks have no generation. A unique matching opening from each source
    // corroborates one activation. Repeated same-session/same-source openings
    // cannot be paired by time: retain the unmatched hook evidence separately.
    for hook in starts.iter().copied().filter(|f| generation(f).is_none()) {
        let matches: Vec<_> = groups
            .iter()
            .enumerate()
             .filter(|(_, g)| generation(g.first()).is_some()
                && same_runtime(g.first(), hook)
                && g.first().refs.session_id == hook.refs.session_id
                && (source(g.first()).is_none() || signature(g.first(), hook)))
            .map(|(i, _)| i)
            .collect();
        let unique_hook = starts
            .iter()
            .filter(|f| generation(f).is_none() && signature(f, hook))
            .count()
            == 1;
        if matches.len() == 1 && unique_hook {
            if let Some(id) = &hook.refs.execution_id {
                groups[matches[0]].members.insert(id.clone());
            }
            groups[matches[0]].starts.push(hook);
        } else if matches.is_empty() {
            groups.push(Group {
                starts: vec![hook],
                members: hook.refs.execution_id.clone().into_iter().collect(),
            });
        }
    }
    let native_count = groups.len();
    let mut pending_inventory = BTreeSet::new();
    for inventory in facts.iter().filter(|f| kind(f) == Some(K::Inventory)) {
        let matches: Vec<_> = groups[..native_count]
            .iter()
            .enumerate()
            .filter(|(_, g)| {
                same_runtime(g.first(), inventory)
                    && g.first().refs.session_id == inventory.refs.session_id
                    // Exact inventory ends belong to the old attachment.
                    && inventory.refs.execution_id.as_ref().and_then(|id|state.activation_candidates.get(id)).is_none_or(|e|e.end_reasons.is_empty())
                    // A later positive inventory pass can recover from a
                    // lagging switch row without reviving the ended native ID.
                    && !facts.iter().any(|end| matches!(end.payload, FactPayload::ExecutionEnded { .. })
                        && end.refs.execution_id.as_ref().is_some_and(|id|g.members.contains(id))
                        && before(end, inventory))
            })
            .map(|(i, _)| i)
            .collect();
        if matches.len() == 1 {
            if let Some(id) = &inventory.refs.execution_id {
                groups[matches[0]].members.insert(id.clone());
            }
        } else if matches.is_empty() {
            groups.push(Group {
                starts: vec![inventory],
                members: inventory.refs.execution_id.clone().into_iter().collect(),
            });
        }
        if matches.len() > 1 && let Some(id) = &inventory.refs.execution_id
            && binding_ids.iter().any(|binding| state.binding_executions.get(binding) == Some(id)) {
            pending_inventory.insert(id.clone());
        }
        // A pending binding retains its original inventory row as an UNKNOWN
        // foreign-key anchor, never another live attachment.
    }
    if structural {
    for id in execution_ids {
        state.executions.remove(id);
        state.execution_aliases.remove(id);
    }
    for group in &groups {
        let Some(id) = group.representative(state) else {
            continue;
        };
        let Some(mut execution) = state.activation_candidates.get(&id).cloned() else {
            continue;
        };
        for member in &group.members {
            state.execution_aliases.insert(member.clone(), id.clone());
            if let Some(raw) = state.activation_candidates.get(member) {
                execution.attachments.extend(raw.attachments.clone());
                execution.end_reasons.extend(raw.end_reasons.clone());
            }
        }
        if groups.iter().any(|later| {
            same_runtime(group.first(), later.first())
                && ordered_before(state, sessions, group.first(), later.first())
                && group.members.is_disjoint(&later.members)
        }) {
            execution
                .end_reasons
                .insert("NATIVE_SESSION_SWITCHED".into());
        }
        for end in facts.iter().filter(|f| {
            kind(f) == Some(K::End)
                && same_runtime(f, group.first())
                && f.refs.session_id == group.first().refs.session_id
        }) {
            if !verified_turn(state, end) {
                continue;
            }
            let matched = if let Some(expected_generation) = generation(end) {
                let owners: Vec<_> = groups
                    .iter()
                    .filter(|g| {
                        same_runtime(g.first(), end)
                            && g.first().refs.session_id == end.refs.session_id
                            && g.starts.iter().any(|s| {
                                generation(s) == Some(expected_generation) && before(s, end)
                            })
                    })
                    .collect();
                let newest: Vec<_> = owners
                    .iter()
                    .filter(|g| !owners.iter().any(|later| before(g.first(), later.first())))
                    .collect();
                newest.len() == 1 && newest[0].members == group.members
            } else {
                groups.iter().filter(|g|same_runtime(g.first(),end) && g.first().refs.session_id==end.refs.session_id).count()==1
                    && group.starts.iter().any(|s|source(s)==Some("startup"))
                    // A legacy end has no provider order. Only an initial
                    // startup uniquely identifies the opening in this runtime.
                    && !starts.iter().any(|s|same_runtime(s,end) && s.refs.session_id==end.refs.session_id && source(s).is_some_and(|source|source!="startup"))
            };
            if matched && let FactPayload::ExecutionEnded { reason } = &end.payload {
                execution.end_reasons.insert(reason.clone());
            }
        }
        state.executions.insert(id, execution);
    }
    for id in pending_inventory {
        if let Some(raw) = state.activation_candidates.get(&id) {
            let mut pending = raw.clone();
            pending.presence = threadspace_contracts::projection::ExecutionPresence::Unknown;
            state.executions.insert(id, pending);
        }
    }
    }
    let mut owners: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for turn in facts.iter().filter(|f| kind(f) == Some(K::Turn)) {
        let Some(turn_id) = &turn.refs.turn_id else {
            continue;
        };
        let ids = owners.entry(turn_id.clone()).or_default();
        if !verified_turn(state, turn) {
            continue;
        }
        let inherited = turn.provenance == EvidenceClass::HostRead;
        let matching: Vec<_> = groups
            .iter()
            .filter(|g| {
                same_runtime(g.first(), turn)
                    && g.first().refs.session_id == turn.refs.session_id
                    && g.starts.iter().any(|s| {
                        generation(s).is_some()
                            && (generation(s) == generation(turn) && before(s, turn)
                                || inherited && ordered_before(state, sessions, s, turn))
                    })
            })
            .collect();
        let newest: Vec<_> = matching
            .iter()
            .filter(|g| {
                !matching
                    .iter()
                    .any(|later| ordered_before(state, sessions, g.first(), later.first()))
            })
            .collect();
        if newest.len() == 1
            && let Some(id) = newest[0].representative(state)
        {
            ids.insert(id);
        }
    }
    for (id, ids) in owners {
        if let Some(turn) = state.turns.get_mut(&id) {
            turn.execution_ids = ids;
        }
    }
    for id in binding_ids {
        let Some(raw) = state.activation_bindings.get(id) else { continue };
        let source = state
            .binding_executions
            .get(id)
            .unwrap_or(&raw.execution_id);
        if let Some(target) = state.execution_aliases.get(source) {
            if let Some(binding) = state.bindings.get_mut(id) {
                binding.execution_id = target.clone();
            }
        } else if state.activation_candidates.contains_key(source)
            && let Some(binding) = state.bindings.get_mut(id) {
            binding.execution_id = source.clone();
            binding.valid = false;
            binding.invalidation_reason = Some("PENDING_ACTIVATION".into());
        }
    }
}
fn verified_turn(state: &CanonicalState, fact: &ResolvedFact) -> bool {
    if fact.provenance == EvidenceClass::ProviderEvent {
        return true;
    }
    let (Some(session), Some(process), Some(ownership), Some(point)) = (
        &fact.refs.session_id,
        &fact.refs.process_id,
        &fact.native.observer_ownership,
        &fact.causal,
    ) else {
        return false;
    };
    state.sessions.get(session).is_some_and(|s| {
        s.ownership_proofs
            .iter()
            .any(|p| p.process_id == *process && p.ownership == *ownership)
            && s.links.iter().any(|l| {
                l.qualified
                    && l.process_id.as_ref() == Some(process)
                    && l.ownership.as_ref() == Some(ownership)
                    && l.point
                        .as_ref()
                        .is_some_and(|p| compare(p, point) == CausalOrder::Before)
            })
    })
}

/// Reload bootstraps explicitly name predecessor epochs. Follow only those
/// native relations, including several reloads; a new UUID is not a start.
fn ordered_before(state: &CanonicalState, sessions: &BTreeSet<String>, a: &ResolvedFact, b: &ResolvedFact) -> bool {
    if before(a, b) {
        return true;
    }
    let (Some(a_point), Some(b_point)) = (&a.causal, &b.causal) else {
        return false;
    };
    let Some(native) = &a_point.native_key else {
        return false;
    };
    let mut epochs = BTreeSet::from([b_point.source_epoch.clone()]);
    loop {
        let mut predecessors = BTreeSet::new();
        for session in sessions.iter().filter_map(|id|state.sessions.get(id))
            .filter(|s| Some(&s.namespace_id) == b.refs.namespace_id.as_ref())
        {
            for link in session
                .links
                .iter()
                .filter(|l| l.process_id == b.refs.process_id)
            {
                if let Some(point) = &link.point
                    && epochs.contains(&point.source_epoch)
                {
                    if point.source_epoch == b_point.source_epoch
                        && compare(point, b_point) != CausalOrder::Before
                    {
                        continue;
                    }
                    if point.native_predecessor_keys.contains(native) {
                        return true;
                    }
                    for key in &point.native_predecessor_keys {
                        if let Some(epoch) = key.strip_prefix("observer-epoch:") {
                            predecessors.insert(epoch.to_owned());
                        }
                    }
                }
            }
        }
        let old = epochs.len();
        epochs.extend(predecessors);
        if old == epochs.len() {
            return false;
        }
    }
}

/// Existing journals already contain inventory openings. Recover their
/// original attach evidence instead of creating a parallel native opening.
pub fn promote_inventory(state: &mut CanonicalState, execution_ids: &BTreeSet<String>, binding_ids: &BTreeSet<String>) -> Vec<ResolvedFact> {
    let mut added = Vec::new();
    use threadspace_contracts::canonical::fact::{CanonicalRefs, NativeActivation, NativeRefs};
    use threadspace_contracts::canonical::keys::{NativeExecutionRef, NativeSessionRef};
    let records: Vec<_> = execution_ids.iter().filter_map(|id|state.executions.get(id))
        .filter(|e| {
            e.activation_ref.starts_with("process:")
                && !state.activation_candidates.contains_key(&e.id)
        })
        .cloned()
        .collect();
    for record in records {
        let Some(session) = state.sessions.get(&record.session_id) else {
            continue;
        };
        let Some(namespace) = state.namespaces.get(&session.namespace_id) else {
            continue;
        };
        let Some(process) = record
            .process_id
            .as_ref()
            .and_then(|id| state.processes.get(id))
        else {
            continue;
        };
        let Some(attach) = record.attachments.iter().next() else {
            continue;
        };
        let fact = ResolvedFact {
            fact_id: crate::ids::derived_id(&format!(
                "inventory-evidence:{}:{}",
                record.id, attach.observation_id
            )),
            observation_id: attach.observation_id.clone(),
            fact_index: attach.fact_index,
            refs: CanonicalRefs {
                namespace_id: Some(namespace.id.clone()),
                session_id: Some(session.id.clone()),
                actor_id: Some(record.actor_id.clone()),
                process_id: Some(process.id.clone()),
                execution_id: Some(record.id.clone()),
                ..CanonicalRefs::default()
            },
            native: NativeRefs {
                session: Some(NativeSessionRef {
                    provider: namespace.provider.clone(),
                    profile_ref: namespace.profile_ref.clone(),
                    native_session_id: session.native_session_id.clone(),
                }),
                process: Some(process.key.clone()),
                execution: Some(NativeExecutionRef::Activation {
                    activation_ref: record.activation_ref.clone(),
                }),
                activation: Some(NativeActivation {
                    kind: K::Inventory,
                    entry_sequence: None,
                    start_source: None,
                    generation: None,
                }),
                ..NativeRefs::default()
            },
            provenance: attach.provenance,
            causal: attach.point.clone(),
            payload_version: threadspace_contracts::canonical::FACT_PAYLOAD_VERSION,
            payload: FactPayload::ExecutionAttached {
                mode: attach.mode,
                presence: attach.presence,
                native_runtime_id: attach.native_runtime_id.clone(),
                controlling_device: attach.controlling_device,
            },
        };
        added.push(fact.clone());
        state.activation_facts.insert(format!("{}:{}", fact.observation_id, fact.fact_index), fact);
        for binding in binding_ids.iter().filter_map(|id|state.bindings.get(id)).filter(|b|b.execution_id == record.id)
        {
            state
                .binding_executions
                .entry(binding.id.clone())
                .or_insert_with(|| record.id.clone());
            state
                .activation_bindings
                .entry(binding.id.clone())
                .or_insert_with(|| binding.clone());
        }
        state
            .activation_candidates
            .insert(record.id.clone(), record);
    }
    added
}
