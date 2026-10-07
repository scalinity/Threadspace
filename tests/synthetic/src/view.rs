//! Native-key queries over a canonical state, for scenario expectations.

use threadspace_contracts::canonical::keys::NativeActorRef;
use threadspace_contracts::canonical::records::{
    AttentionRecord, AttentionScope, CanonicalState, ExecutionRecord, ResolutionKind,
    SessionRecord, SurfaceBindingRecord, TurnRecord,
};

pub struct View<'a> {
    pub state: &'a CanonicalState,
}

impl<'a> View<'a> {
    pub fn new(state: &'a CanonicalState) -> Self {
        Self { state }
    }

    pub fn sessions_named(&self, native: &str) -> usize {
        self.state
            .sessions
            .values()
            .filter(|s| s.native_session_id == native)
            .count()
    }

    pub fn session(&self, native: &str) -> Option<&'a SessionRecord> {
        self.state
            .sessions
            .values()
            .find(|s| s.native_session_id == native)
    }

    pub fn turn(&self, session: &str, agent: Option<&str>, turn: &str) -> Option<&'a TurnRecord> {
        let session = self.session(session)?;
        self.state.turns.values().find(|t| {
            t.session_id == session.id
                && t.native_turn_id.as_deref() == Some(turn)
                && self.state.actors.get(&t.actor_id).is_some_and(|a| match (&a.native, agent) {
                    (NativeActorRef::Principal, None) => true,
                    (NativeActorRef::Agent { native_agent_id }, Some(agent)) => native_agent_id == agent,
                    _ => false,
                })
        })
    }

    pub fn output_item(&self, turn: &TurnRecord) -> Option<&'a AttentionRecord> {
        self.state.attention.values().find(|a| {
            matches!(&a.scope, AttentionScope::TurnOutput { turn_id } if *turn_id == turn.id)
        })
    }

    pub fn execution(&self, session: &str, activation: &str) -> Option<&'a ExecutionRecord> {
        let session = self.session(session)?;
        self.state
            .executions
            .values()
            .find(|e| e.session_id == session.id && e.activation_ref == activation)
    }

    pub fn bindings(&self, execution: &ExecutionRecord) -> Vec<&'a SurfaceBindingRecord> {
        self.state
            .bindings
            .values()
            .filter(|b| b.execution_id == execution.id)
            .collect()
    }

    pub fn wait_items(&self, session: &str) -> Vec<&'a AttentionRecord> {
        let Some(session) = self.session(session) else {
            return Vec::new();
        };
        self.state
            .attention
            .values()
            .filter(|a| {
                a.session_id == session.id
                    && matches!(a.scope, AttentionScope::SessionWaitCategory { .. })
            })
            .collect()
    }

    pub fn request_item(&self, session: &str, request: &str) -> Option<&'a AttentionRecord> {
        let session = self.session(session)?;
        self.state.attention.values().find(|a| {
            a.session_id == session.id
                && matches!(&a.scope, AttentionScope::ExactRequest { native_request_id } if native_request_id == request)
        })
    }
}

pub fn resolved_by(item: &AttentionRecord, kind: ResolutionKind) -> bool {
    item.resolutions.iter().any(|c| c.kind == kind)
}
