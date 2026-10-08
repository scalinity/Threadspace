//! Qualification-only fixtures (absent from release builds): attention on an
//! existing Session, synthetic streaming changes and oversized stores. Each
//! is admitted canonically like any other observation.

use threadspace_contracts::canonical::fact::{
    Delivery, EvidenceClass, FactPayload, NativeFactDraft, NativeRefs, TurnOutcome,
};
use threadspace_contracts::canonical::keys::NativeSessionRef;

use crate::{
    Change, FIXTURE_NATIVE_SESSION, FIXTURE_PROFILE, FIXTURE_PROVIDER, Journal, JournalError,
    RaisedAttention,
};

const SOURCE_QUALIFICATION: &str = "qualification";
const SYNTHETIC_PROFILE: &str = "m0c-synthetic";
/// The summary prefix cleanup recognizes; summaries are bounded to 120 chars.
const SUMMARY_PREFIX: &str = "Qualification turn completed — ";

fn synthetic_session(native: String) -> NativeSessionRef {
    NativeSessionRef {
        provider: FIXTURE_PROVIDER.into(),
        profile_ref: SYNTHETIC_PROFILE.into(),
        native_session_id: native,
    }
}

fn identified(session: NativeSessionRef, display_name: String) -> NativeFactDraft {
    NativeFactDraft {
        refs: NativeRefs {
            session: Some(session),
            ..NativeRefs::default()
        },
        provenance: EvidenceClass::Derived,
        causal: None,
        payload: FactPayload::SessionIdentified {
            display_name: Some(display_name),
            start_source: None,
        },
    }
}

impl Journal {
    /// Commits a completed turn, its owner attention item and a PENDING
    /// notification intent on `session_id` (any Session, including a
    /// provider-observed one) or on the fixture Session when `None`.
    pub fn raise_attention_on(
        &mut self,
        label: &str,
        session_id: Option<&str>,
        now_ms: i64,
    ) -> Result<RaisedAttention, JournalError> {
        let state = &self.engine.state;
        let session = match session_id {
            Some(id) => state.sessions.get(id),
            None => state.sessions.values().find(|s| {
                s.native_session_id == FIXTURE_NATIVE_SESSION
                    && state
                        .namespaces
                        .get(&s.namespace_id)
                        .is_some_and(|n| n.provider == FIXTURE_PROVIDER && n.profile_ref == FIXTURE_PROFILE)
            }),
        };
        let Some(session) = session else {
            return Err(JournalError::NotFound {
                entity: "session",
                id: session_id.unwrap_or("fixture").to_owned(),
            });
        };
        let Some(namespace) = state.namespaces.get(&session.namespace_id) else {
            return Err(JournalError::NotFound {
                entity: "namespace",
                id: session.namespace_id.clone(),
            });
        };
        let native = NativeSessionRef {
            provider: namespace.provider.clone(),
            profile_ref: namespace.profile_ref.clone(),
            native_session_id: session.native_session_id.clone(),
        };
        let target_session = session.id.clone();
        let observation_id = self.allocate_id();
        let label: String = label.chars().take(120 - SUMMARY_PREFIX.chars().count()).collect();
        let summary = format!("{SUMMARY_PREFIX}{label}");
        let draft = NativeFactDraft {
            refs: NativeRefs {
                session: Some(native),
                turn: Some(format!("m0c-qualification-turn-{observation_id}")),
                ..NativeRefs::default()
            },
            provenance: EvidenceClass::Derived,
            causal: None,
            payload: FactPayload::TurnOutcomeObserved {
                outcome: TurnOutcome::Completed,
                reason: None,
                summary: Some(summary.clone()),
            },
        };
        let payload = serde_json::json!({ "label": label, "sessionId": target_session });
        let (cursor, output) = self.admit_internal(
            observation_id,
            SOURCE_QUALIFICATION,
            "QUALIFY_ATTENTION_RAISED",
            &payload,
            &[draft],
            Vec::new(),
            Delivery::Live,
            now_ms,
            now_ms,
            |_, _, _, _| Ok(()),
        )?;
        let state = &self.engine.state;
        let intent = output
            .new_outbox
            .iter()
            .filter_map(|id| state.outbox.get(id))
            .find_map(|outbox| crate::canonical::intent_for(state, outbox))
            .ok_or_else(|| JournalError::Invalid {
                detail: "qualification attention produced no notification intent".into(),
            })?;
        let intent = crate::NotificationIntent {
            title: format!("Qualification: {label}"),
            body: summary,
            ..intent
        };
        Ok(RaisedAttention {
            change: Change {
                cursor,
                session_ids: vec![target_session],
                attention_ids: vec![intent.attention_id.clone()],
            },
            intent,
        })
    }

    /// One synthetic committed change: upserts fixture Session
    /// `m0c-stream-<slot>` with a display name naming `sequence`.
    pub fn synthetic_change(
        &mut self,
        run_id: &str,
        slot: u32,
        sequence: u64,
        now_ms: i64,
    ) -> Result<Change, JournalError> {
        let name = format!("Stream {slot} · change {sequence} · run {run_id}");
        let name: String = name.chars().take(120).collect();
        let draft = identified(synthetic_session(format!("m0c-stream-{slot}")), name);
        let payload = serde_json::json!({ "runId": run_id, "slot": slot, "sequence": sequence });
        let observation_id = self.allocate_id();
        let (cursor, output) = self.admit_internal(
            observation_id,
            SOURCE_QUALIFICATION,
            "QUALIFY_SYNTHETIC_CHANGE",
            &payload,
            &[draft],
            Vec::new(),
            Delivery::Live,
            now_ms,
            now_ms,
            |_, _, _, _| Ok(()),
        )?;
        Ok(Change {
            cursor,
            session_ids: output.changed.sessions.into_iter().collect(),
            attention_ids: Vec::new(),
        })
    }

    /// Adds `count` synthetic fixture Sessions with `name_bytes`-long names in
    /// one transaction (oversized-snapshot fixtures). Names are bounded by
    /// the canonical label limit; a longer request is padded to that limit.
    pub fn populate_synthetic(
        &mut self,
        count: u32,
        name_bytes: u32,
        now_ms: i64,
    ) -> Result<Change, JournalError> {
        let observation_id = self.allocate_id();
        let padding = "x".repeat(name_bytes.min(100) as usize);
        let drafts: Vec<NativeFactDraft> = (0..count)
            .map(|index| {
                identified(
                    synthetic_session(format!("m0c-populate-{observation_id}-{index}")),
                    format!("Populated {index} {padding}"),
                )
            })
            .collect();
        let payload = serde_json::json!({ "count": count, "nameBytes": name_bytes });
        let (cursor, output) = self.admit_internal(
            observation_id,
            SOURCE_QUALIFICATION,
            "QUALIFY_POPULATED",
            &payload,
            &drafts,
            Vec::new(),
            Delivery::Live,
            now_ms,
            now_ms,
            |_, _, _, _| Ok(()),
        )?;
        Ok(Change {
            cursor,
            session_ids: output.changed.sessions.into_iter().collect(),
            attention_ids: Vec::new(),
        })
    }
}
