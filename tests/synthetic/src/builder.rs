//! Native-shaped synthetic histories. A scenario is a list of steps in their
//! causal (capture) order plus the delivery constraints that any valid
//! delivery must respect. Captured sequences, observation IDs and clock
//! values are fixed at build time, so a delivery permutation reorders
//! arrival only: never the evidence itself.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use threadspace_contracts::canonical::command::OwnerAction;
use threadspace_contracts::canonical::envelope::{
    CaptureClock, ClockQuality, OBSERVATION_SCHEMA_VERSION, ObservationEnvelope, ProcessRole,
    ProcessSample, SequenceMeaning,
};
use threadspace_contracts::canonical::keys::NativeSessionRef;
use threadspace_contracts::route::ProcessKey;
use threadspace_state_engine::ids::derived_id;
use threadspace_state_engine::synthetic::{ADAPTER_ID, ADAPTER_VERSION};

use crate::rng::VirtualClock;

pub const DEFAULT_SOURCE: &str = "synthetic.mod";
pub const DEFAULT_EPOCH: &str = "mod-epoch-1";
pub const BOOT: &str = "boot-A";

/// What an owner command targets, by native scope (the way the owner's UI
/// names an item it can see).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    TurnOutput {
        session: NativeSessionRef,
        agent: Option<String>,
        turn: String,
    },
    Request {
        session: NativeSessionRef,
        request: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerStep {
    pub command_id: String,
    pub target: Target,
    pub action: OwnerAction,
    pub at_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Observe(Box<ObservationEnvelope>),
    Owner(OwnerStep),
}

impl Step {
    pub fn label(&self) -> String {
        match self {
            Self::Observe(envelope) => envelope.native_event.clone(),
            Self::Owner(owner) => format!("owner:{}", owner.command_id),
        }
    }
}

/// A scenario's expectation over the final canonical state.
pub type Expect = fn(&threadspace_contracts::canonical::records::CanonicalState) -> Result<(), String>;

pub struct Scenario {
    pub name: String,
    pub family: &'static str,
    pub steps: Vec<Step>,
    /// (a, b): step `a` must be delivered before step `b`.
    pub constraints: Vec<(usize, usize)>,
    pub expect: Expect,
}

pub fn session(profile: &str, native: &str) -> NativeSessionRef {
    NativeSessionRef {
        provider: "synthetic".into(),
        profile_ref: profile.into(),
        native_session_id: native.into(),
    }
}

/// A local ProcessKey; the endpoint is filled in at admission.
pub fn process(pid: u32, start_seconds: u64) -> ProcessKey {
    ProcessKey {
        endpoint_id: String::new(),
        boot_id: BOOT.into(),
        pid,
        start_seconds: start_seconds.to_string(),
        start_microseconds: 0,
    }
}

pub struct Builder {
    name: String,
    family: &'static str,
    clock: VirtualClock,
    steps: Vec<Step>,
    constraints: Vec<(usize, usize)>,
    sequences: BTreeMap<(String, String), u64>,
}

pub struct Obs<'a> {
    builder: &'a mut Builder,
    envelope: ObservationEnvelope,
    ordered: bool,
    sequence: Option<u64>,
}

impl Builder {
    pub fn new(name: impl Into<String>, family: &'static str) -> Self {
        Self {
            name: name.into(),
            family,
            clock: VirtualClock::new(),
            steps: Vec::new(),
            constraints: Vec::new(),
            sequences: BTreeMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// The capture sequence recorded for an observation step.
    pub fn step_sequence(&self, index: usize) -> Option<u64> {
        match self.steps.get(index)? {
            Step::Observe(envelope) => envelope.source_sequence.as_deref()?.parse().ok(),
            Step::Owner(_) => None,
        }
    }

    /// Advances the virtual clock (delayed upstream work, for example).
    pub fn wait_ms(&mut self, ms: i64) {
        self.clock.advance(ms);
    }

    pub fn obs(&mut self, session: Option<&NativeSessionRef>, event: &str) -> Obs<'_> {
        let index = self.steps.len();
        let wall = self.clock.tick();
        let envelope = ObservationEnvelope {
            schema_version: OBSERVATION_SCHEMA_VERSION,
            observation_id: derived_id(&format!("synthetic-observation|{}|{index}", self.name)),
            source_id: DEFAULT_SOURCE.into(),
            source_epoch: DEFAULT_EPOCH.into(),
            source_sequence: None,
            sequence_meaning: None,
            callback_entry_sequence: None,
            callback_result_sequence: None,
            adapter_id: ADAPTER_ID.into(),
            adapter_version: ADAPTER_VERSION.into(),
            provider_version: Some("synthetic-1".into()),
            native_event: event.into(),
            session_key: session.cloned(),
            actor_native_id: None,
            native_turn_id: None,
            native_prompt_id: None,
            native_occurrence_id: None,
            activation_ref: None,
            captured_at: CaptureClock {
                endpoint_id: None,
                boot_id: Some(BOOT.into()),
                monotonic_ns: Some(((wall - VirtualClock::EPOCH_MS) * 1_000_000).to_string()),
                wall_time_ms: wall,
                clock_quality: ClockQuality::LocalMonotonic,
            },
            evidence: Vec::new(),
            payload: json!({}),
        };
        Obs {
            builder: self,
            envelope,
            ordered: true,
            sequence: None,
        }
    }

    pub fn owner(&mut self, command_id: &str, target: Target, action: OwnerAction, after: &[usize]) -> usize {
        let index = self.steps.len();
        let at_ms = self.clock.tick();
        self.steps.push(Step::Owner(OwnerStep {
            command_id: command_id.into(),
            target,
            action,
            at_ms,
        }));
        for &a in after {
            self.constraints.push((a, index));
        }
        // Owner commands keep their own order: the owner issued them in turn.
        let previous: Vec<usize> = (0..index)
            .filter(|&i| matches!(self.steps[i], Step::Owner(_)))
            .collect();
        for i in previous {
            self.constraints.push((i, index));
        }
        index
    }

    /// Repeats an earlier observation with the same UUID (a retry/duplicate).
    pub fn duplicate(&mut self, of: usize) -> usize {
        let step = self.steps[of].clone();
        self.steps.push(step);
        self.steps.len() - 1
    }

    pub fn before(&mut self, a: usize, b: usize) {
        self.constraints.push((a, b));
    }

    pub fn build(self, expect: Expect) -> Scenario {
        Scenario {
            name: self.name,
            family: self.family,
            steps: self.steps,
            constraints: self.constraints,
            expect,
        }
    }
}

impl Obs<'_> {
    pub fn turn(mut self, turn: &str) -> Self {
        self.envelope.native_turn_id = Some(turn.into());
        self
    }

    pub fn agent(mut self, agent: &str) -> Self {
        self.envelope.actor_native_id = Some(agent.into());
        self
    }

    pub fn prompt(mut self, prompt: &str) -> Self {
        self.envelope.native_prompt_id = Some(prompt.into());
        self
    }

    pub fn occurrence(mut self, occurrence: &str) -> Self {
        self.envelope.native_occurrence_id = Some(occurrence.into());
        self
    }

    pub fn activation(mut self, activation: &str) -> Self {
        self.envelope.activation_ref = Some(activation.into());
        self
    }

    pub fn provider(mut self, key: ProcessKey, executable: &str, device: Option<u32>) -> Self {
        self.envelope.evidence.push(ProcessSample {
            role: ProcessRole::Provider,
            key,
            parent_pid: None,
            executable: Some(executable.into()),
            controlling_device: device,
        });
        self
    }

    pub fn payload(mut self, payload: Value) -> Self {
        self.envelope.payload = payload;
        self
    }

    pub fn source(mut self, source: &str, epoch: &str) -> Self {
        self.envelope.source_id = source.into();
        self.envelope.source_epoch = epoch.into();
        self
    }

    /// No qualified sequence: the evidence has no causal point.
    pub fn unordered(mut self) -> Self {
        self.ordered = false;
        self
    }

    /// An explicit capture sequence (an upstream order that differs from
    /// this capture's position, for example).
    pub fn sequence(mut self, sequence: u64) -> Self {
        self.sequence = Some(sequence);
        self
    }

    pub fn push(self) -> usize {
        let Obs {
            builder,
            mut envelope,
            ordered,
            sequence,
        } = self;
        if ordered {
            let key = (envelope.source_id.clone(), envelope.source_epoch.clone());
            let counter = builder.sequences.entry(key).or_insert(0);
            *counter += 1;
            let value = sequence.unwrap_or(*counter);
            *counter = (*counter).max(value);
            envelope.source_sequence = Some(value.to_string());
            envelope.sequence_meaning = Some(SequenceMeaning::ObserverCapture);
        }
        builder.steps.push(Step::Observe(Box::new(envelope)));
        builder.steps.len() - 1
    }
}
