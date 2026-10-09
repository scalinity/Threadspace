//! The semantic projection (SPEC §5.5): a state normalized by native keys,
//! relation identity and stable attention scope, for comparing independently
//! admitted arrival permutations.
//!
//! It excludes exactly what may legitimately differ between two admissions
//! of the same native history: allocated UUID values (every reference is
//! rewritten to its native key), ingest cursors and revisions, receipt and
//! creation timestamps, per-session activation numbers (allocation order),
//! display names, summaries and route records (presentation), surface
//! status (diagnostic), and outbox history (a suppressed intent depends on
//! when evidence arrived; only the still-eligible intent set is compared).
//! Identity relationships, lifecycle, outcomes, uncertainty, attention,
//! commands and coverage are all included.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use threadspace_contracts::canonical::keys::NativeActorRef;
use threadspace_contracts::canonical::records::{AttentionScope, CanonicalState, OutboxState, WaitScopeRecord};

use crate::hash::{canonical_json, sha256_hex};
use crate::wait;

struct Names<'a> {
    state: &'a CanonicalState,
    episodes: BTreeMap<String, (String, u32)>,
}

fn text(value: &Value) -> String {
    value.to_string()
}

impl<'a> Names<'a> {
    fn new(state: &'a CanonicalState) -> Self {
        let mut episodes = BTreeMap::new();
        for wait in state.waits.values() {
            for episode in &wait.episodes {
                episodes.insert(episode.episode_id.clone(), (wait.key.clone(), episode.index));
            }
        }
        Self { state, episodes }
    }

    fn namespace(&self, id: &str) -> Value {
        self.state.namespaces.get(id).map_or(Value::Null, |n| {
            json!(["namespace", n.provider, n.profile_ref])
        })
    }

    fn session(&self, id: &str) -> Value {
        self.state.sessions.get(id).map_or(Value::Null, |s| {
            json!([self.namespace(&s.namespace_id), s.native_session_id])
        })
    }

    fn actor(&self, id: &str) -> Value {
        self.state.actors.get(id).map_or(Value::Null, |a| {
            let native = match &a.native {
                NativeActorRef::Principal => json!("principal"),
                NativeActorRef::Agent { native_agent_id } => json!(["agent", native_agent_id]),
            };
            json!([self.session(&a.session_id), native])
        })
    }

    fn process(&self, id: &str) -> Value {
        self.state.processes.get(id).map_or(Value::Null, |p| {
            json!([p.key.boot_id, p.key.pid, p.key.start_seconds, p.key.start_microseconds])
        })
    }

    fn execution(&self, id: &str) -> Value {
        self.state.executions.get(id).map_or(Value::Null, |e| {
            json!([self.session(&e.session_id), self.actor(&e.actor_id), e.activation_ref])
        })
    }

    fn turn(&self, id: &str) -> Value {
        self.state.turns.get(id).map_or(Value::Null, |t| {
            json!([self.session(&t.session_id), self.actor(&t.actor_id), t.native_turn_id])
        })
    }

    fn input(&self, id: &str) -> Value {
        self.state.inputs.get(id).map_or(Value::Null, |i| {
            json!([self.session(&i.session_id), i.native_key])
        })
    }

    fn activity(&self, id: &str) -> Value {
        self.state.activities.get(id).map_or(Value::Null, |a| {
            json!([self.session(&a.session_id), a.native_occurrence_id])
        })
    }

    fn surface(&self, id: &str) -> Value {
        self.state.surfaces.get(id).map_or(Value::Null, |s| {
            let n = &s.native;
            json!([n.surface_kind, n.app_generation, n.locator, n.device_number, n.surface_generation])
        })
    }

    fn binding(&self, id: &str) -> Value {
        self.state.bindings.get(id).map_or(Value::Null, |b| {
            json!([self.execution(&b.execution_id), self.surface(&b.surface_id)])
        })
    }

    fn wait(&self, key: &str) -> Value {
        self.state.waits.get(key).map_or(Value::Null, |w| {
            json!([
                self.session(&w.session_id),
                w.actor_id.as_deref().map(|a| self.actor(a)),
                w.execution_id.as_deref().map(|e| self.execution(e)),
                w.turn_id.as_deref().map(|t| self.turn(t)),
                w.category,
                w.generation,
            ])
        })
    }

    fn request(&self, key: &str) -> Value {
        self.state.requests.get(key).map_or(Value::Null, |r| {
            json!([self.session(&r.session_id), r.native_request_id])
        })
    }

    fn attention(&self, id: &str) -> Value {
        self.state.attention.get(id).map_or(Value::Null, |a| {
            let scope = match &a.scope {
                AttentionScope::TurnOutput { turn_id } => json!(["TURN_OUTPUT", self.turn(turn_id)]),
                AttentionScope::ExactRequest { native_request_id } => {
                    json!(["EXACT_REQUEST", native_request_id])
                }
                AttentionScope::SessionWaitCategory { episode_id, .. } => match self.episodes.get(episode_id) {
                    Some((wait, index)) => json!(["SESSION_WAIT_CATEGORY", self.wait(wait), index]),
                    None => json!(["SESSION_WAIT_CATEGORY", null]),
                },
                AttentionScope::OwnerDecision { decision_id } => json!(["OWNER_DECISION", decision_id]),
            };
            json!([self.session(&a.session_id), scope])
        })
    }
}

fn map<'a, I>(items: I) -> Value
where
    I: Iterator<Item = (Value, Value)> + 'a,
{
    let mut out = serde_json::Map::new();
    for (key, value) in items {
        out.insert(text(&key), value);
    }
    Value::Object(out)
}

fn sorted(mut values: Vec<Value>) -> Value {
    values.sort_by_key(text);
    Value::Array(values)
}

/// Each owner decision by the evidence it covered (native causal points and
/// the count of positives without one: what the owner was shown) and the
/// episodes whose items it applies to now. Episode ordinals alone would
/// equate decisions made on different evidence.
fn owner_decisions(scope: &WaitScopeRecord) -> Value {
    let owner: Vec<(u32, wait::OwnerState<'_>)> = wait::partition(scope)
        .iter()
        .map(|(index, episode)| (*index, wait::owner_state(&scope.owner_decisions, episode)))
        .collect();
    scope
        .owner_decisions
        .iter()
        .map(|d| {
            json!({
                "command": d.command_id,
                "action": d.action,
                "covers": d.positives,
                "coversUnordered": d.unordered,
                "governs": owner.iter().filter(|(_, state)| state.applies(d)).map(|(i, _)| *i).collect::<Vec<_>>(),
            })
        })
        .collect()
}

#[allow(clippy::too_many_lines)]
pub fn projection(state: &CanonicalState) -> Value {
    let n = Names::new(state);
    json!({
        "sessions": map(state.sessions.values().map(|s| (n.session(&s.id), json!({
            "recordState": s.record_state,
            "startSources": s.start_sources,
            "fixture": s.fixture,
            "inventory": s.inventory.as_ref().map(|i| json!({ "present": i.present, "row": i.row })),
            "executionPresence": s.execution_presence,
            "observation": s.observation,
            "turnState": s.turn_state,
            "linkConflict": s.link_conflict,
            "observerTier": s.observer_tier,
        })))),
        "actors": map(state.actors.values().map(|a| (n.actor(&a.id), json!({
            "role": a.role,
            "agentTypes": a.agent_types,
            "runsEnded": a.runs_ended,
        })))),
        "relations": sorted(state.relations.iter().map(|r| json!([
            n.actor(&r.actor_id), n.actor(&r.related_actor_id), r.relation,
        ])).collect()),
        "processes": map(state.processes.values().map(|p| (n.process(&p.id), json!({
            "images": p.images,
            "currentExecutable": p.current_executable,
            "exited": p.exited,
        })))),
        "executions": map(state.executions.values().map(|e| (n.execution(&e.id), json!({
            "process": e.process_id.as_deref().map(|p| n.process(p)),
            "mode": e.mode,
            "attached": e.attached,
            "nativeRuntimeId": e.native_runtime_id,
            "controllingDevice": e.controlling_device,
            "endReasons": e.end_reasons,
            "presence": e.presence,
            "attachmentConflict": e.attachment_conflict,
        })))),
        "turns": map(state.turns.values().map(|t| (n.turn(&t.id), json!({
            "executions": sorted(t.execution_ids.iter().map(|e| n.execution(e)).collect()),
            "identityKind": t.identity_kind,
            "ownerFacing": t.owner_facing,
            "started": t.started,
            "stepped": t.stepped,
            "activitySeen": t.activity_seen,
            "queuedInputs": sorted(t.queued_inputs.iter().map(|i| n.input(i)).collect()),
            "startedByInputs": sorted(t.started_by_inputs.iter().map(|i| n.input(i)).collect()),
            "outcomes": t.outcomes,
            "outcomeReasons": t.outcome_reasons,
            "outputReady": t.output_ready,
            "outputPoints": t.output_points,
            "state": t.state,
            "outcomeConflict": t.outcome_conflict,
            "pendingOutcomes": sorted(t.pending_outcomes.iter().map(|p| json!([
                p.outcome,
                p.reason,
                p.point,
                p.process_id.as_deref().map(|id| n.process(id)),
            ])).collect()),
        })))),
        "inputs": map(state.inputs.values().map(|i| (n.input(&i.id), json!({
            "actor": n.actor(&i.actor_id),
            "origin": i.origin,
            "submission": i.submission,
            "activeTurn": i.active_turn_id.as_deref().map(|t| n.turn(t)),
            "acceptances": i.acceptances,
            "rejections": i.rejections,
            "startedTurns": sorted(i.started_turns.iter().map(|t| n.turn(t)).collect()),
        })))),
        "activities": map(state.activities.values().map(|a| (n.activity(&a.id), json!({
            "actor": n.actor(&a.actor_id),
            "turn": a.turn_id.as_deref().map(|t| n.turn(t)),
            "toolCategories": a.tool_categories,
            "proposed": a.proposed,
            "started": a.started,
            "finished": a.finished,
            "permissionChecked": a.permission_checked,
        })))),
        "surfaces": sorted(state.surfaces.keys().map(|s| n.surface(s)).collect()),
        "bindings": map(state.bindings.values().map(|b| (n.binding(&b.id), json!({
            "method": b.method,
            "executableIdentity": b.executable_identity,
            "invalidations": b.invalidations,
            "valid": b.valid,
            "invalidationReason": b.invalidation_reason,
        })))),
        "waits": map(state.waits.values().map(|w| (n.wait(&w.key), json!({
            "turn": w.turn_id.as_deref().map(|t| n.turn(t)),
            "subtypes": w.subtypes,
            "positives": w.positives,
            "clears": w.clears,
            "unorderedPositives": w.unordered_positives,
            "unorderedClears": w.unordered_clears,
            "episodes": w.episodes.iter().map(|e| json!({
                "index": e.index, "active": e.active, "uncertain": e.uncertain,
            })).collect::<Vec<_>>(),
            // Durable owner decisions, by their covered evidence.
            "ownerDecisions": owner_decisions(w),
        })))),
        "requests": map(state.requests.values().map(|r| (n.request(&r.key), json!({
            "turn": r.turn_id.as_deref().map(|t| n.turn(t)),
            "category": r.category,
            "positive": r.positive,
            "resolved": r.resolved,
        })))),
        // An item whose wait episode a late clear emptied is delivery history:
        // it exists only because the clear arrived after the positive.
        "attention": map(state.attention.values().filter(|a| match &a.scope {
            AttentionScope::SessionWaitCategory { episode_id, .. } => n.episodes.contains_key(episode_id),
            _ => true,
        }).map(|a| (n.attention(&a.id), json!({
            "actor": a.actor_id.as_deref().map(|x| n.actor(x)),
            "turn": a.turn_id.as_deref().map(|t| n.turn(t)),
            "category": a.category,
            "priority": a.priority,
            "acknowledgements": a.acknowledgements,
            "resolutions": a.resolutions,
            "snoozedUntilMs": a.snoozed_until_ms,
        })))),
        "eligibleOutbox": sorted(state.outbox.values()
            .filter(|o| matches!(o.state, OutboxState::Pending | OutboxState::Held))
            .map(|o| n.attention(&o.attention_id))
            .collect()),
        "frontiers": map(state.frontiers.values().map(|f| (json!([
            n.session(&f.session_id), n.actor(&f.actor_id), f.source_id, f.source_epoch, f.order_domain,
        ]), json!({ "maxSequence": f.max_sequence, "predecessorKeys": f.predecessor_keys })))),
        "coverage": map(state.coverage.values().map(|c| (json!([c.source_id, c.source_epoch]), json!({
            "meaning": c.meaning, "seen": c.seen, "gaps": c.gaps, "reportedGaps": c.reported_gaps,
        })))),
        // A command on a wait item is compared as its scope's owner decision
        // (above): which item showed that evidence depended on arrival.
        "commands": map(state.commands.values().map(|c| (json!(c.command_id), json!({
            "attention": match state.attention.get(&c.attention_id).map(|a| &a.scope) {
                Some(AttentionScope::SessionWaitCategory { .. }) => json!("WAIT_DECISION"),
                _ => n.attention(&c.attention_id),
            },
            "action": c.action,
        })))),
    })
}

pub fn semantic_hash(state: &CanonicalState) -> String {
    sha256_hex(&canonical_json(&projection(state)))
}
