//! Identity resolution: the admission step between pure normalization and
//! pure reduction (SPEC §5.1). Native keys resolve to recorded canonical IDs;
//! a never-seen native object gets a newly allocated ID, recorded as an
//! assignment that commits with the facts. A draft whose ownership cannot be
//! proven stays unresolved; it never borrows a current session.

use std::collections::BTreeMap;

use threadspace_contracts::canonical::fact::{
    CanonicalFactKind, CanonicalRefs, NativeFactDraft, ResolvedFact,
};
use threadspace_contracts::canonical::keys::{NativeActorRef, NativeExecutionRef};
use threadspace_contracts::canonical::records::CanonicalState;
use threadspace_contracts::canonical::FACT_PAYLOAD_VERSION;

use crate::ids::Allocator;
use crate::keys;
use crate::validate;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Entity {
    Namespace,
    Session,
    Actor,
    Process,
    Execution,
    Turn,
    Input,
    Activity,
    Surface,
    Binding,
}

impl Entity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Namespace => "namespace",
            Self::Session => "session",
            Self::Actor => "actor",
            Self::Process => "process",
            Self::Execution => "execution",
            Self::Turn => "turn",
            Self::Input => "input",
            Self::Activity => "activity",
            Self::Surface => "surface",
            Self::Binding => "binding",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "namespace" => Self::Namespace,
            "session" => Self::Session,
            "actor" => Self::Actor,
            "process" => Self::Process,
            "execution" => Self::Execution,
            "turn" => Self::Turn,
            "input" => Self::Input,
            "activity" => Self::Activity,
            "surface" => Self::Surface,
            "binding" => Self::Binding,
            _ => return None,
        })
    }
}

/// Recorded native key → canonical ID assignments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdentityIndex {
    map: BTreeMap<String, String>,
}

impl IdentityIndex {
    pub fn get(&self, native_key: &str) -> Option<&str> {
        self.map.get(native_key).map(String::as_str)
    }

    pub fn insert(&mut self, native_key: String, id: String) {
        self.map.insert(native_key, id);
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn apply(&mut self, assignments: &[Assignment]) {
        for assignment in assignments {
            self.insert(assignment.native_key.clone(), assignment.id.clone());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub native_key: String,
    pub entity: Entity,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unresolved {
    pub fact_index: u32,
    pub kind: CanonicalFactKind,
    pub reason: &'static str,
}

/// Resolves the drafts of one admission batch. New assignments stay in its
/// overlay until the caller commits them.
pub struct Resolver<'a> {
    index: &'a IdentityIndex,
    pending: BTreeMap<String, Assignment>,
    /// Every new native key, in allocation order.
    order: Vec<String>,
    /// How much of `order` `drain_new` already returned.
    drained: usize,
    allocator: &'a mut dyn Allocator,
    endpoint_id: &'a str,
}

/// Which references a fact kind needs before it may be reduced.
fn needs(kind: CanonicalFactKind) -> &'static [&'static str] {
    use CanonicalFactKind as K;
    match kind {
        K::SessionIdentified
        | K::SessionRecordChanged
        | K::ObservationLinkChanged
        | K::ProviderSnapshotObserved
        | K::RouteResultRecorded
        | K::ResponseBoundaryObserved
        | K::PermissionCheckObserved
        | K::WaitStateObserved => &["session"],
        K::ExecutionAttached | K::ExecutionEnded | K::SurfaceBindingUnproven => &["execution"],
        K::ProcessObserved | K::ProcessExitObserved => &["process"],
        K::ObserverOwnershipCorroborated => &["session", "process"],
        K::InputSubmitted | K::InputAccepted | K::InputRejected => &["session", "input"],
        K::TurnStarted | K::TurnStepObserved | K::OutputReady | K::TurnOutcomeObserved => {
            &["session", "turn"]
        }
        K::ActivityProposed | K::ActivityStarted | K::ActivityFinished => &["session", "activity"],
        K::RequestResolved => &["session", "request"],
        K::ActorIdentified | K::ActorRunEnded => &["actor"],
        K::ActorRelationObserved => &["actor", "related_actor"],
        K::SurfaceBindingRecorded | K::SurfaceBindingInvalidated => &["binding"],
        K::ObservationGapDetected => &[],
        K::AttentionAcknowledged
        | K::AttentionResolved
        | K::AttentionSnoozed
        | K::NotificationDeliveryRecorded => &["attention"],
    }
}

fn present(refs: &CanonicalRefs, name: &str) -> bool {
    match name {
        "session" => refs.session_id.is_some(),
        "actor" => refs.actor_id.is_some(),
        "related_actor" => refs.related_actor_id.is_some(),
        "process" => refs.process_id.is_some(),
        "execution" => refs.execution_id.is_some(),
        "turn" => refs.turn_id.is_some(),
        "input" => refs.input_id.is_some(),
        "activity" => refs.activity_id.is_some(),
        "request" => refs.request.is_some(),
        "binding" => refs.binding_id.is_some(),
        "attention" => refs.attention_id.is_some(),
        _ => false,
    }
}

fn missing_reason(name: &str) -> &'static str {
    match name {
        "session" => "MISSING_SESSION",
        "actor" => "MISSING_ACTOR",
        "related_actor" => "MISSING_RELATED_ACTOR",
        "process" => "MISSING_PROCESS",
        "execution" => "MISSING_EXECUTION",
        "turn" => "MISSING_TURN",
        "input" => "MISSING_INPUT",
        "activity" => "MISSING_ACTIVITY",
        "request" => "MISSING_REQUEST",
        "binding" => "MISSING_BINDING",
        "attention" => "UNKNOWN_ATTENTION",
        _ => "MISSING_REFERENCE",
    }
}

impl<'a> Resolver<'a> {
    pub fn new(
        index: &'a IdentityIndex,
        allocator: &'a mut dyn Allocator,
        endpoint_id: &'a str,
    ) -> Self {
        Self {
            index,
            pending: BTreeMap::new(),
            order: Vec::new(),
            drained: 0,
            allocator,
            endpoint_id,
        }
    }

    pub fn allocate_id(&mut self) -> String {
        self.allocator.allocate()
    }

    fn id_for(&mut self, entity: Entity, native_key: String) -> String {
        if let Some(id) = self.index.get(&native_key) {
            return id.to_owned();
        }
        if let Some(assignment) = self.pending.get(&native_key) {
            return assignment.id.clone();
        }
        let id = self.allocator.allocate();
        self.order.push(native_key.clone());
        self.pending.insert(
            native_key.clone(),
            Assignment {
                native_key,
                entity,
                id: id.clone(),
            },
        );
        id
    }

    /// Resolves one observation's drafts, in order. Facts keep their draft
    /// positions; unresolved drafts are reported with a bounded reason.
    pub fn resolve(
        &mut self,
        state: &CanonicalState,
        observation_id: &str,
        drafts: &[NativeFactDraft],
    ) -> (Vec<ResolvedFact>, Vec<Unresolved>) {
        let mut facts = Vec::new();
        let mut unresolved = Vec::new();
        for (index, draft) in drafts.iter().enumerate() {
            let index = index as u32;
            let kind = draft.payload.kind();
            if let Err(reason) = validate::draft(draft) {
                unresolved.push(Unresolved {
                    fact_index: index,
                    kind,
                    reason,
                });
                continue;
            }
            match self.refs(state, draft) {
                Ok(refs) => {
                    if let Some(name) = needs(kind).iter().find(|name| !present(&refs, name)) {
                        unresolved.push(Unresolved {
                            fact_index: index,
                            kind,
                            reason: missing_reason(name),
                        });
                        continue;
                    }
                    let mut native = draft.refs.clone();
                    if let Some(process) = native.process.as_mut()
                        && process.endpoint_id.is_empty()
                    {
                        process.endpoint_id = self.endpoint_id.to_owned();
                    }
                    facts.push(ResolvedFact {
                        fact_id: self.allocator.allocate(),
                        observation_id: observation_id.to_owned(),
                        fact_index: index,
                        refs,
                        native,
                        provenance: draft.provenance,
                        causal: draft.causal.clone(),
                        payload_version: FACT_PAYLOAD_VERSION,
                        payload: draft.payload.clone(),
                    });
                }
                Err(reason) => unresolved.push(Unresolved {
                    fact_index: index,
                    kind,
                    reason,
                }),
            }
        }
        (facts, unresolved)
    }

    fn refs(
        &mut self,
        state: &CanonicalState,
        draft: &NativeFactDraft,
    ) -> Result<CanonicalRefs, &'static str> {
        let native = &draft.refs;
        let mut refs = CanonicalRefs::default();
        let endpoint = self.endpoint_id.to_owned();

        if let Some(session) = &native.session {
            let namespace = self.id_for(
                Entity::Namespace,
                keys::namespace(&session.provider, &endpoint, &session.profile_ref),
            );
            let session_id = self.id_for(
                Entity::Session,
                keys::session(&namespace, &session.native_session_id),
            );
            refs.namespace_id = Some(namespace);
            refs.session_id = Some(session_id);
        }

        // Actor-scoped references default to the principal actor of the
        // session; a subordinate is only ever named by its native agent ID.
        let actor_scoped = native.actor.is_some()
            || native.turn.is_some()
            || native.input.is_some()
            || native.activity.is_some()
            || matches!(native.execution, Some(NativeExecutionRef::Activation { .. }));
        if actor_scoped {
            let session_id = refs.session_id.clone().ok_or("MISSING_SESSION")?;
            let actor = native.actor.clone().unwrap_or(NativeActorRef::Principal);
            refs.actor_id = Some(self.id_for(Entity::Actor, keys::actor(&session_id, &actor)));
        }
        if let Some(related) = &native.related_actor {
            let session_id = refs.session_id.clone().ok_or("MISSING_SESSION")?;
            refs.related_actor_id =
                Some(self.id_for(Entity::Actor, keys::actor(&session_id, related)));
        }

        if let Some(process) = &native.process {
            let mut process = process.clone();
            if process.endpoint_id.is_empty() {
                process.endpoint_id.clone_from(&endpoint);
            }
            refs.process_id = Some(self.id_for(Entity::Process, keys::process(&process)));
        }

        match &native.execution {
            Some(NativeExecutionRef::Activation { activation_ref }) => {
                let session_id = refs.session_id.clone().ok_or("MISSING_SESSION")?;
                let actor_id = refs.actor_id.clone().ok_or("MISSING_ACTOR")?;
                refs.execution_id = Some(self.id_for(
                    Entity::Execution,
                    keys::execution(&session_id, &actor_id, activation_ref),
                ));
            }
            Some(NativeExecutionRef::Canonical { execution_id }) => {
                let execution = state
                    .executions
                    .get(execution_id)
                    .ok_or("UNKNOWN_EXECUTION")?;
                if refs
                    .session_id
                    .as_ref()
                    .is_some_and(|session| *session != execution.session_id)
                {
                    return Err("EXECUTION_SESSION_MISMATCH");
                }
                refs.session_id = Some(execution.session_id.clone());
                refs.namespace_id = state
                    .sessions
                    .get(&execution.session_id)
                    .map(|session| session.namespace_id.clone());
                refs.actor_id = Some(execution.actor_id.clone());
                if refs.process_id.is_none() {
                    refs.process_id.clone_from(&execution.process_id);
                }
                refs.execution_id = Some(execution_id.clone());
            }
            None => {}
        }

        if let Some(turn) = &native.turn {
            let session_id = refs.session_id.clone().ok_or("MISSING_SESSION")?;
            let actor_id = refs.actor_id.clone().ok_or("MISSING_ACTOR")?;
            refs.turn_id = Some(self.id_for(Entity::Turn, keys::turn(&session_id, &actor_id, turn)));
        }
        if let Some(input) = &native.input {
            let session_id = refs.session_id.clone().ok_or("MISSING_SESSION")?;
            refs.input_id = Some(self.id_for(Entity::Input, keys::input(&session_id, input)));
        }
        if let Some(activity) = &native.activity {
            let session_id = refs.session_id.clone().ok_or("MISSING_SESSION")?;
            refs.activity_id =
                Some(self.id_for(Entity::Activity, keys::activity(&session_id, activity)));
        }
        if let Some(request) = &native.request {
            if refs.session_id.is_none() {
                return Err("MISSING_SESSION");
            }
            refs.request = Some(request.clone());
        }
        if let Some(surface) = &native.surface {
            refs.surface_id = Some(self.id_for(Entity::Surface, keys::surface(&endpoint, surface)));
        }
        if let (Some(execution), Some(surface)) = (&refs.execution_id, &refs.surface_id) {
            refs.binding_id = Some(self.id_for(Entity::Binding, keys::binding(execution, surface)));
        }
        if let Some(attention) = &native.attention {
            let item = state.attention.get(attention).ok_or("UNKNOWN_ATTENTION")?;
            refs.attention_id = Some(attention.clone());
            refs.session_id = Some(item.session_id.clone());
        }
        Ok(refs)
    }

    /// Assignments allocated since the last drain, in allocation order. They
    /// stay visible to later lookups in this batch, and `into_assignments`
    /// still returns them.
    pub fn drain_new(&mut self) -> Vec<Assignment> {
        let fresh = self.order[self.drained..]
            .iter()
            .filter_map(|key| self.pending.get(key).cloned())
            .collect();
        self.drained = self.order.len();
        fresh
    }

    /// The new assignments in allocation order.
    pub fn into_assignments(mut self) -> Vec<Assignment> {
        self.order
            .iter()
            .filter_map(|key| self.pending.remove(key))
            .collect()
    }
}
