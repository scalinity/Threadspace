//! The Claude observer mod's records (`threadspace-observer`, SPEC §11.4) as
//! canonical facts: pure over the envelope `threadspace-hook mod-batch`
//! built. Authority (D-0005, D-0010):
//!
//! - the profile is qualified only when the provider process's kernel-read
//!   executable is a qualified build (`profiles`); an unqualified observer
//!   yields only an unqualified link report, never lifecycle;
//! - an identity the engine stamped (`classic.SessionStart`) gives
//!   provider-event provenance; one read from the host (`$.session.id()`,
//!   as after a reload) gives host-read provenance, whose turn outcome the
//!   reducer applies only once kernel/inventory evidence corroborates the
//!   provider process, and which drafts no accepted input or actor;
//! - lifecycle (turn start, outcome, accepted input, spawned actor) needs
//!   engine dispatch and a settled core trace, which only a result shows.
//!
//! Every record is read on its own, with the session, actor and turn the mod
//! froze at callback entry, so a delayed result is never reattributed.

use serde_json::Value;
use threadspace_contracts::canonical::causal::CausalPoint;
use threadspace_contracts::canonical::envelope::{ObservationEnvelope, ProcessRole};
use threadspace_contracts::canonical::fact::{
    AcceptanceProof, ActivityResult, ActorRelationKind, ActorRole, EvidenceClass, FactPayload,
    InputOrigin, NativeFactDraft, NativeRefs, ObserverOwnership, TurnOutcome,
};
use threadspace_contracts::canonical::keys::NativeActorRef;
use threadspace_contracts::projection::ObservationState;
use threadspace_state_engine::normalize::{Normalized, retain};

use crate::profiles::{observer_profile, version_from_executable};

pub const ADAPTER_ID: &str = "threadspace-observer";
const QUEUE_DROPPED_EVENT: &str = "observer.queue-dropped";
const ORDER_DOMAIN: &str = "observer-capture";
const RETAINED_KEYS: &[&str] = &[
    "phase", "dispatchPlugin", "dispatchTier", "engineDispatch", "sessionIdSource",
    "sessionGeneration", "detail", "droppedRecords", "ownershipEpoch", "ownershipGeneration",
    "ownershipStatus", "ownershipProofToken", "currentSessionId", "currentSessionGeneration", "currentSessionIdSource",
];

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// The native key every point of one observer load carries, which a later
/// load's bootstrap names as its predecessor.
fn epoch_key(epoch: &str) -> String {
    format!("observer-epoch:{epoch}")
}

fn origin(kind: Option<&str>) -> InputOrigin {
    match kind {
        Some("composer") => InputOrigin::HumanComposer,
        Some("bridge") => InputOrigin::HumanBridge,
        Some("sdk") => InputOrigin::Sdk,
        Some("task-notification") => InputOrigin::TaskNotification,
        Some("scheduled-trigger") => InputOrigin::Scheduled,
        Some("plugin") => InputOrigin::Plugin,
        Some("peer") => InputOrigin::Peer,
        _ => InputOrigin::Unclassified,
    }
}

fn outcome(reason: Option<&str>) -> Option<TurnOutcome> {
    match reason? {
        "answer" => Some(TurnOutcome::Completed),
        "aborted" => Some(TurnOutcome::Interrupted),
        "error" => Some(TurnOutcome::Failed),
        "refusal" => Some(TurnOutcome::Refused),
        _ => None,
    }
}

/// What a record proves, read once.
struct Record<'a> {
    envelope: &'a ObservationEnvelope,
    phase: &'a str,
    /// The provider runs a qualified build, and the mod's own version claim
    /// (bootstrap) does not contradict the kernel's.
    qualified: bool,
    version: Option<String>,
    /// Dispatched by the engine's core (host-stamped `next.origin`).
    engine: bool,
    /// The trace beneath settled in the engine's core.
    settled: bool,
    /// The session identity was stamped by the engine, not read from the host.
    engine_identity: bool,
}

impl Record<'_> {
    fn provenance(&self) -> EvidenceClass {
        if self.engine_identity && self.engine {
            EvidenceClass::ProviderEvent
        } else {
            EvidenceClass::HostRead
        }
    }

    fn point(&self, sequence: Option<&String>, predecessor: Option<&str>) -> Option<CausalPoint> {
        Some(CausalPoint {
            source_id: self.envelope.source_id.clone(),
            source_epoch: self.envelope.source_epoch.clone(),
            order_domain: ORDER_DOMAIN.to_owned(),
            sequence: Some(sequence?.clone()),
            native_key: Some(epoch_key(&self.envelope.source_epoch)),
            native_predecessor_keys: predecessor.map(epoch_key).into_iter().collect(),
        })
    }

    fn causal(&self) -> Option<CausalPoint> {
        self.point(self.envelope.source_sequence.as_ref(), None)
    }

    fn link(&self, refs: &NativeRefs, link: ObservationState, provenance: EvidenceClass, causal: Option<CausalPoint>) -> NativeFactDraft {
        NativeFactDraft {
            refs: NativeRefs {
                actor: None,
                turn: None,
                observer_ownership: (self.envelope.native_event == "ownership.seal")
                    .then(|| refs.observer_ownership.clone()).flatten(),
                ..refs.clone()
            },
            provenance,
            causal,
            payload: FactPayload::ObservationLinkChanged {
                link,
                qualified: self.qualified,
                version: self.version.clone(),
            },
        }
    }
}

pub fn normalize(envelope: &ObservationEnvelope) -> Normalized {
    let retained = retain(&envelope.payload, RETAINED_KEYS);
    let done = |drafts: Vec<NativeFactDraft>, unsupported: Option<String>| Normalized {
        drafts,
        retained: retained.clone(),
        unsupported,
    };
    if envelope.native_event == QUEUE_DROPPED_EVENT {
        let dropped = envelope.payload.get("droppedRecords").and_then(Value::as_u64).unwrap_or(0);
        return done(
            vec![NativeFactDraft {
                refs: NativeRefs::default(),
                provenance: EvidenceClass::Derived,
                causal: None,
                payload: FactPayload::ObservationGapDetected {
                    domain: "observer-queue".into(),
                    detail: format!("{dropped} records evicted before delivery"),
                },
            }],
            None,
        );
    }
    let Some(session) = envelope.session_key.clone() else {
        return done(Vec::new(), Some("NO_SESSION: no session identity".into()));
    };
    let payload = &envelope.payload;
    let detail = payload.get("detail").unwrap_or(&Value::Null);
    let provider = envelope.evidence.iter().find(|s| s.role == ProcessRole::Provider);
    let version = provider
        .and_then(|s| s.executable.as_deref())
        .and_then(version_from_executable);
    let claimed = detail.get("version").and_then(|v| text(v, "version"));
    let record = Record {
        envelope,
        phase: text(payload, "phase").unwrap_or_default(),
        qualified: version.as_deref().is_some_and(|v| observer_profile(v).is_some())
            && claimed.is_none_or(|c| Some(c) == version.as_deref()),
        version,
        engine: payload.get("engineDispatch").and_then(Value::as_bool) == Some(true)
            && text(payload, "dispatchPlugin") == Some("engine")
            && text(payload, "dispatchTier") == Some("core"),
        settled: detail.get("core").and_then(|c| c.get("coreSettled")).and_then(Value::as_bool) == Some(true),
        engine_identity: text(payload, "sessionIdSource") == Some("classic.SessionStart"),
    };
    let refs = NativeRefs {
        session: Some(session),
        actor: envelope
            .actor_native_id
            .clone()
            .map(|native_agent_id| NativeActorRef::Agent { native_agent_id }),
        process: provider.map(|s| s.key.clone()),
        turn: envelope.native_turn_id.clone(),
        observer_ownership: (|| {
            let source_epoch = text(payload, "ownershipEpoch")?;
            if source_epoch != envelope.source_epoch {
                return None;
            }
            let session_generation = payload.get("ownershipGeneration")?.as_u64()?;
            if session_generation == 0 {
                return None;
            }
            Some(ObserverOwnership {
                source_epoch: source_epoch.into(),
                session_generation,
                proof_token: text(payload, "ownershipProofToken")?.into(),
                executable_identity: provider?.executable.clone()?,
            })
        })(),
        ..NativeRefs::default()
    };
    let draft = |refs: NativeRefs, provenance: EvidenceClass, payload: FactPayload| NativeFactDraft {
        refs,
        provenance,
        causal: record.causal(),
        payload,
    };
    let lifecycle = record.qualified && record.engine && record.settled;
    let event = envelope.native_event.as_str();
    let drafts = match (event, record.phase) {
        // A seal is only a host-read claim. The reducer additionally requires
        // the independent native proof and a later qualified core TurnStart;
        // an interceptable returned token alone creates no authority.
        ("ownership.seal", "result") if record.qualified => vec![record.link(
            &NativeRefs { actor: None, turn: None, ..refs.clone() },
            ObservationState::Current,
            EvidenceClass::HostRead,
            record.causal(),
        )],
        // Bootstrap: placed first in its load (its entry), after every report
        // of the load it says it replaced. Its identity is the engine's when
        // classic.SessionStart came first, otherwise a host read.
        ("session.start", "bootstrap") => vec![record.link(
            &refs,
            ObservationState::Current,
            record.provenance(),
            record.point(envelope.callback_entry_sequence.as_ref(), text(detail, "predecessorEpoch")),
        )],
        ("classic.SessionStart", "entry") if record.qualified => vec![draft(
            NativeRefs { actor: None, turn: None, ..refs.clone() },
            record.provenance(),
            FactPayload::SessionIdentified {
                display_name: None,
                start_source: text(detail, "source").map(str::to_owned),
            },
        )],
        ("classic.SessionStart", "result") => {
            let provenance = if record.engine && record.settled { record.provenance() } else { EvidenceClass::HostRead };
            vec![record.link(&refs, ObservationState::Current, provenance, record.causal())]
        }
        ("session.end", "entry") if !matches!(text(detail, "reason"), Some("clear" | "compact" | "resume")) => {
            vec![record.link(&refs, ObservationState::Disconnected, record.provenance(), record.causal())]
        }
        ("prompt.submit", "entry") if record.qualified => vec![draft(
            NativeRefs {
                input: envelope.native_prompt_id.clone(),
                turn: text(detail, "activeTurnIdAtSubmission").map(str::to_owned),
                ..refs.clone()
            },
            record.provenance(),
            FactPayload::InputSubmitted {
                origin: origin(detail.get("origin").and_then(|o| text(o, "kind"))),
                submission: None,
            },
        )],
        ("prompt.submit", "result") if record.qualified && record.engine_identity => {
            let input = NativeRefs {
                input: envelope.native_prompt_id.clone(),
                turn: None,
                ..refs.clone()
            };
            if text(detail, "outcome") == Some("dropped") {
                vec![draft(input, record.provenance(), FactPayload::InputRejected { reason: "DROPPED".into() })]
            } else if record.engine && record.settled {
                let proof = AcceptanceProof {
                    engine_dispatch: record.engine,
                    core_settled: record.settled,
                    original_origin_protected: detail.get("resultOrigin").and_then(|o| text(o, "kind")).is_some(),
                    dropped: false,
                };
                vec![draft(input, record.provenance(), FactPayload::InputAccepted { proof })]
            } else {
                Vec::new()
            }
        }
        ("turn.start", "result") if lifecycle && refs.turn.is_some() => {
            vec![draft(refs.clone(), record.provenance(), FactPayload::TurnStarted {})]
        }
        ("turn.step", "result") if record.qualified && record.engine && refs.turn.is_some() => {
            vec![draft(refs.clone(), record.provenance(), FactPayload::TurnStepObserved {})]
        }
        // Only an engine-dispatched completion that settled in core is a
        // native outcome; a plugin's lifecycle-shaped report is none.
        ("turn.complete", "result") if lifecycle && refs.turn.is_some() => match outcome(text(detail, "reason")) {
            Some(outcome) => vec![draft(
                refs.clone(),
                record.provenance(),
                FactPayload::TurnOutcomeObserved {
                    outcome,
                    reason: text(detail, "reason").map(str::to_owned),
                    summary: None,
                },
            )],
            None => Vec::new(),
        },
        ("tool.call" | "tool.check", _) if record.qualified && envelope.native_occurrence_id.is_some() => {
            let activity = NativeRefs {
                activity: envelope.native_occurrence_id.clone(),
                ..refs.clone()
            };
            let tool = text(detail, "tool").unwrap_or("unknown").to_owned();
            match (event, record.phase) {
                ("tool.call", "entry") => vec![draft(activity, record.provenance(), FactPayload::ActivityStarted { tool_category: tool })],
                ("tool.call", "result") => {
                    let result = match text(detail, "resultKind") {
                        Some("result") => ActivityResult::Success,
                        _ => ActivityResult::Failure,
                    };
                    vec![draft(activity, record.provenance(), FactPayload::ActivityFinished { tool_category: tool, result })]
                }
                ("tool.check", "entry") => vec![draft(activity, record.provenance(), FactPayload::PermissionCheckObserved { tool_category: tool })],
                _ => Vec::new(),
            }
        }
        // A spawned actor exists only once a settled core spawn returned it;
        // a downstream answer naming a plausible agent is no proof.
        ("agent.spawn", "result")
            if lifecycle && record.engine_identity && text(detail, "outcome") == Some("started") =>
        {
            match text(detail, "agentId") {
                Some(agent) => {
                    let child = NativeActorRef::Agent { native_agent_id: agent.to_owned() };
                    let parent = refs.actor.clone().unwrap_or(NativeActorRef::Principal);
                    vec![
                        draft(
                            NativeRefs { actor: Some(child.clone()), turn: None, ..refs.clone() },
                            record.provenance(),
                            FactPayload::ActorIdentified { role: ActorRole::Subordinate, agent_type: None },
                        ),
                        draft(
                            NativeRefs { actor: Some(child), related_actor: Some(parent), turn: None, ..refs.clone() },
                            record.provenance(),
                            FactPayload::ActorRelationObserved { relation: ActorRelationKind::ImmediateParent },
                        ),
                    ]
                }
                None => Vec::new(),
            }
        }
        // Attach/detach is client metadata without activation identity, so
        // it is no execution attach (D-0010); errors and abandoned streams
        // are no outcome.
        _ => Vec::new(),
    };
    done(drafts, None)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use threadspace_contracts::canonical::envelope::{
        CaptureClock, ClockQuality, OBSERVATION_SCHEMA_VERSION, ProcessSample, SequenceMeaning,
    };
    use threadspace_contracts::canonical::fact::CanonicalFactKind;
    use threadspace_contracts::canonical::keys::NativeSessionRef;
    use threadspace_contracts::route::ProcessKey;

    use super::*;

    const EPOCH: &str = "6d1c7f0e-1111-4aaa-8bbb-000000000002";
    const QUALIFIED: &str = "/Users/u/.local/share/claude/versions/2.1.295";

    struct Rec {
        event: &'static str,
        phase: &'static str,
        executable: &'static str,
        plugin: &'static str,
        identity: &'static str,
        session: &'static str,
        detail: Value,
    }

    fn rec(event: &'static str, phase: &'static str, detail: Value) -> Rec {
        Rec { event, phase, executable: QUALIFIED, plugin: "engine", identity: "classic.SessionStart", session: "S1", detail }
    }

    fn settled() -> Value {
        json!({ "links": 1, "endPlugin": "engine", "endTier": "core", "endOutcome": "returned", "coreSettled": true })
    }

    fn envelope(r: &Rec) -> ObservationEnvelope {
        let engine = r.plugin == "engine";
        ObservationEnvelope {
            schema_version: OBSERVATION_SCHEMA_VERSION,
            observation_id: "00000000-0000-4000-8000-000000000009".into(),
            source_id: "claude.observer".into(),
            source_epoch: EPOCH.into(),
            source_sequence: Some("4".into()),
            sequence_meaning: Some(SequenceMeaning::ObserverCapture),
            callback_entry_sequence: Some("3".into()),
            callback_result_sequence: (r.phase != "entry").then(|| "4".into()),
            adapter_id: ADAPTER_ID.into(),
            adapter_version: "0.1.0".into(),
            provider_version: None,
            native_event: r.event.into(),
            session_key: Some(NativeSessionRef {
                provider: "claude".into(),
                profile_ref: "claude-cli:/Users/u/.claude".into(),
                native_session_id: r.session.into(),
            }),
            actor_native_id: None,
            native_turn_id: Some("turn-1".into()),
            native_prompt_id: (r.event == "prompt.submit").then(|| format!("observer:{EPOCH}:3")),
            native_occurrence_id: Some("toolu-1".into()),
            activation_ref: None,
            captured_at: CaptureClock {
                endpoint_id: None,
                boot_id: None,
                monotonic_ns: None,
                wall_time_ms: 1,
                clock_quality: ClockQuality::ReceiptOnly,
            },
            evidence: vec![ProcessSample {
                role: ProcessRole::Provider,
                key: ProcessKey {
                    endpoint_id: String::new(),
                    boot_id: "boot".into(),
                    pid: 100,
                    start_seconds: "1791000000".into(),
                    start_microseconds: 7,
                },
                parent_pid: None,
                executable: Some(r.executable.into()),
                controlling_device: None,
            }],
            payload: json!({
                "phase": r.phase, "dispatchPlugin": r.plugin, "dispatchTier": if engine { "core" } else { "user" },
                "engineDispatch": engine, "sessionIdSource": r.identity, "sessionGeneration": 1, "detail": r.detail,
                "prompt": "SECRET",
            }),
        }
    }

    fn drafts(r: Rec) -> Vec<NativeFactDraft> {
        normalize(&envelope(&r)).drafts
    }

    fn kinds(drafts: &[NativeFactDraft]) -> Vec<CanonicalFactKind> {
        drafts.iter().map(|d| d.payload.kind()).collect()
    }

    #[test]
    fn a_settled_engine_completion_is_a_native_outcome() {
        let d = drafts(rec("turn.complete", "result", json!({ "reason": "answer", "isAborted": false, "core": settled() })));
        assert_eq!(kinds(&d), vec![CanonicalFactKind::TurnOutcomeObserved]);
        assert_eq!(d[0].provenance, EvidenceClass::ProviderEvent);
        assert!(matches!(d[0].payload, FactPayload::TurnOutcomeObserved { outcome: TurnOutcome::Completed, .. }));
        assert_eq!(d[0].refs.process.as_ref().map(|k| k.pid), Some(100), "names the kernel-read provider process");
        for (reason, expected) in [("aborted", TurnOutcome::Interrupted), ("error", TurnOutcome::Failed), ("refusal", TurnOutcome::Refused)] {
            let d = drafts(rec("turn.complete", "result", json!({ "reason": reason, "core": settled() })));
            assert!(matches!(&d[0].payload, FactPayload::TurnOutcomeObserved { outcome, .. } if *outcome == expected), "{reason}");
        }
    }

    #[test]
    fn nonengine_turn_complete_creates_no_outcome() {
        let mut forged = rec("turn.complete", "result", json!({ "reason": "answer", "core": settled() }));
        forged.plugin = "forger";
        assert!(drafts(forged).is_empty(), "a plugin's lifecycle-shaped report is none");
        let unsettled = rec("turn.complete", "result", json!({ "reason": "answer", "core": { "coreSettled": false, "endPlugin": "shortcut" } }));
        assert!(drafts(unsettled).is_empty(), "a downstream answer without core is none");
        assert!(drafts(rec("turn.complete", "entry", json!({ "reason": "answer" }))).is_empty(), "the entry shows no settlement");
    }

    #[test]
    fn a_host_read_identity_marks_its_outcome_for_corroboration() {
        let mut reloaded = rec("turn.complete", "result", json!({ "reason": "answer", "core": settled() }));
        reloaded.identity = "session.id";
        let d = drafts(reloaded);
        assert_eq!(kinds(&d), vec![CanonicalFactKind::TurnOutcomeObserved]);
        assert_eq!(d[0].provenance, EvidenceClass::HostRead);
    }

    #[test]
    fn an_unqualified_build_yields_only_an_unqualified_link() {
        for executable in ["/Users/u/.local/share/claude/versions/2.1.292", "/Users/u/.local/share/claude/ClaudeCode.app/Contents/MacOS/claude"] {
            let mut completion = rec("turn.complete", "result", json!({ "reason": "answer", "core": settled() }));
            completion.executable = executable;
            assert!(drafts(completion).is_empty(), "{executable}: no lifecycle");
            let mut start = rec("classic.SessionStart", "result", json!({ "core": settled() }));
            start.executable = executable;
            let d = drafts(start);
            assert!(matches!(d[0].payload, FactPayload::ObservationLinkChanged { qualified: false, .. }), "{executable}");
        }
        let mut contradicted = rec("session.start", "bootstrap", json!({ "hostSessionId": "S1", "version": { "version": "2.1.295" } }));
        contradicted.executable = "/Users/u/.local/share/claude/versions/2.1.294";
        let d = drafts(contradicted);
        assert!(matches!(&d[0].payload, FactPayload::ObservationLinkChanged { qualified: false, version: Some(v), .. } if v == "2.1.294"));
        let lying = rec("session.start", "bootstrap", json!({ "hostSessionId": "S1", "version": { "version": "2.1.291" } }));
        assert!(matches!(drafts(lying)[0].payload, FactPayload::ObservationLinkChanged { qualified: false, .. }), "a claim the kernel contradicts");
    }

    #[test]
    fn a_bootstrap_after_the_engine_identity_keeps_it() {
        // Natively classic.SessionStart can fire before session.start, so the
        // bootstrap's frozen session is already the engine's.
        let d = drafts(rec("session.start", "bootstrap", json!({ "hostSessionId": "S1", "version": { "version": "2.1.295" } })));
        assert_eq!(d[0].provenance, EvidenceClass::ProviderEvent);
        let mut executable = rec("turn.complete", "result", json!({ "reason": "answer", "core": settled() }));
        executable.executable = "/Users/u/.local/share/claude/versions/2.1.295#16777234:152193867";
        assert_eq!(kinds(&drafts(executable)), vec![CanonicalFactKind::TurnOutcomeObserved], "a kernel image identity qualifies");
    }

    #[test]
    fn a_reload_bootstrap_follows_its_predecessor_load() {
        let mut reloaded = rec("session.start", "bootstrap", json!({ "hostSessionId": "S1", "predecessorEpoch": "earlier-epoch", "version": { "version": "2.1.295" } }));
        reloaded.identity = "session.id";
        let d = drafts(reloaded);
        assert_eq!(kinds(&d), vec![CanonicalFactKind::ObservationLinkChanged]);
        assert_eq!(d[0].provenance, EvidenceClass::HostRead, "a host read until corroborated");
        let point = d[0].causal.as_ref().expect("point");
        assert_eq!(point.sequence.as_deref(), Some("3"), "placed at its entry, first in its load");
        assert_eq!(point.native_key.as_deref(), Some(format!("observer-epoch:{EPOCH}").as_str()));
        assert_eq!(point.native_predecessor_keys, vec!["observer-epoch:earlier-epoch".to_owned()]);
        let engine = drafts(rec("classic.SessionStart", "result", json!({ "core": settled() })));
        assert_eq!(engine[0].provenance, EvidenceClass::ProviderEvent);
        assert!(matches!(engine[0].payload, FactPayload::ObservationLinkChanged { link: ObservationState::Current, qualified: true, .. }));
    }

    #[test]
    fn forged_spawn_creates_no_actor() {
        let real = drafts(rec("agent.spawn", "result", json!({ "outcome": "started", "agentId": "a2cca502", "core": settled() })));
        assert_eq!(kinds(&real), vec![CanonicalFactKind::ActorIdentified, CanonicalFactKind::ActorRelationObserved]);
        let shortcut = drafts(rec("agent.spawn", "result", json!({ "outcome": "started", "agentId": "agent-fabricated-1", "core": { "coreSettled": false } })));
        assert!(shortcut.is_empty(), "a plausible agent from a shortcut is no actor");
        let mut reloaded = rec("agent.spawn", "result", json!({ "outcome": "started", "agentId": "a2cca502", "core": settled() }));
        reloaded.identity = "session.id";
        assert!(drafts(reloaded).is_empty(), "a host-read identity drafts no actor");
    }

    #[test]
    fn submissions_record_origin_acceptance_and_drops() {
        let entry = drafts(rec("prompt.submit", "entry", json!({ "origin": { "kind": "composer" }, "activeTurnIdAtSubmission": "turn-0" })));
        assert!(matches!(entry[0].payload, FactPayload::InputSubmitted { origin: InputOrigin::HumanComposer, .. }));
        assert_eq!(entry[0].refs.turn.as_deref(), Some("turn-0"), "the turn running at submission");
        assert_eq!(entry[0].refs.input.as_deref(), Some(format!("observer:{EPOCH}:3").as_str()));
        let accepted = drafts(rec("prompt.submit", "result", json!({ "outcome": "entered", "resultOrigin": { "kind": "composer" }, "core": settled() })));
        assert!(matches!(accepted[0].payload, FactPayload::InputAccepted { proof } if proof.qualified()));
        assert_eq!(accepted[0].refs.input, entry[0].refs.input, "entry and result name one input");
        let dropped = drafts(rec("prompt.submit", "result", json!({ "outcome": "dropped", "core": settled() })));
        assert!(matches!(dropped[0].payload, FactPayload::InputRejected { .. }));
        let mut reloaded = rec("prompt.submit", "result", json!({ "outcome": "entered", "resultOrigin": { "kind": "composer" }, "core": settled() }));
        reloaded.identity = "session.id";
        assert!(drafts(reloaded).is_empty(), "no accepted input at a host-read tier");
    }

    #[test]
    fn tools_are_activity_and_attachments_are_not_executions() {
        assert_eq!(kinds(&drafts(rec("tool.call", "entry", json!({ "tool": "Bash" })))), vec![CanonicalFactKind::ActivityStarted]);
        let finished = drafts(rec("tool.call", "result", json!({ "tool": "Bash", "resultKind": "error", "core": settled() })));
        assert!(matches!(finished[0].payload, FactPayload::ActivityFinished { result: ActivityResult::Failure, .. }));
        assert_eq!(kinds(&drafts(rec("tool.check", "entry", json!({ "tool": "Bash" })))), vec![CanonicalFactKind::PermissionCheckObserved]);
        assert!(drafts(rec("session.attach", "result", json!({ "surface": "terminal" }))).is_empty());
        assert!(drafts(rec("session.detach", "result", json!({ "surface": "terminal" }))).is_empty());
        assert!(drafts(rec("turn.step", "abandoned", json!({ "core": settled() }))).is_empty());
    }

    #[test]
    fn delayed_result_keeps_entry_session() {
        let mut earlier = rec("turn.complete", "result", json!({ "reason": "answer", "core": settled() }));
        earlier.session = "S-A";
        let d = drafts(earlier);
        assert_eq!(d[0].refs.session.as_ref().map(|s| s.native_session_id.as_str()), Some("S-A"), "the session frozen at entry");
    }

    #[test]
    fn session_end_disconnects_except_for_logical_changes() {
        let exit = drafts(rec("session.end", "entry", json!({ "reason": "prompt_input_exit" })));
        assert!(matches!(exit[0].payload, FactPayload::ObservationLinkChanged { link: ObservationState::Disconnected, .. }));
        assert!(drafts(rec("session.end", "entry", json!({ "reason": "clear" }))).is_empty());
    }

    #[test]
    fn only_allowlisted_payload_is_retained() {
        let n = normalize(&envelope(&rec("turn.complete", "result", json!({ "reason": "answer", "core": settled() }))));
        assert!(n.retained.get("prompt").is_none(), "{}", n.retained);
        assert!(n.retained.get("detail").is_some());
    }
}
