//! Qualification-only fixtures (absent from release builds): attention on an
//! existing Session, synthetic streaming changes and oversized stores.

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use uuid::Uuid;

use crate::{Change, FIXTURE_PROVIDER, Journal, JournalError, NotificationIntent, RaisedAttention};

const SOURCE_QUALIFICATION: &str = "qualification";
const SYNTHETIC_PROFILE: &str = "m0c-synthetic";

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
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let target: Option<(String, Option<String>)> = match session_id {
            Some(id) => tx
                .query_row(
                    "SELECT s.id, (SELECT e.id FROM executions e WHERE e.session_id = s.id
                                    ORDER BY e.activation DESC LIMIT 1)
                       FROM sessions s WHERE s.id = ?1",
                    params![id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?,
            None => tx
                .query_row(
                    "SELECT s.id, e.id FROM sessions s JOIN executions e ON e.session_id = s.id
                      WHERE s.fixture = 1 AND s.native_session_id = 'm0a-fixture-session-1'
                      ORDER BY e.activation DESC LIMIT 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?,
        };
        let Some((session_id, execution_id)) = target else {
            return Err(JournalError::NotFound {
                entity: "session",
                id: session_id.unwrap_or("fixture").to_owned(),
            });
        };
        let payload = serde_json::json!({ "label": label, "sessionId": session_id });
        let (observation_id, cursor) = Self::insert_observation(
            &tx,
            SOURCE_QUALIFICATION,
            &self.source_epoch,
            "QUALIFY_ATTENTION_RAISED",
            &payload,
            now_ms,
        )?;
        let turn_id = Uuid::new_v4().to_string();
        let attention_id = Uuid::new_v4().to_string();
        let request_id = Uuid::new_v4().to_string();
        let native_turn = format!("m0c-qualification-turn-{cursor}");
        let summary = format!("Qualification turn completed — {label}");
        tx.execute(
            "INSERT INTO turns (id, session_id, execution_id, native_turn_id, identity_kind, state, created_cursor)
             VALUES (?1, ?2, ?3, ?4, 'LOCAL_PROVISIONAL', 'COMPLETED', ?5)",
            params![turn_id, session_id, execution_id, native_turn, cursor],
        )?;
        tx.execute(
            "INSERT INTO attention_items (id, session_id, turn_id, category, scope_kind, scope_key, priority, summary,
               created_by_observation, created_at_ms, notification_state, revision)
             VALUES (?1, ?2, ?3, 'TURN_COMPLETE', 'TURN_OUTPUT', ?3, 40, ?4, ?5, ?6, 'PENDING', ?7)",
            params![attention_id, session_id, turn_id, summary, observation_id, now_ms, cursor],
        )?;
        tx.execute(
            "INSERT INTO notification_outbox (request_id, attention_id, state, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, 'PENDING', ?3, ?3)",
            params![request_id, attention_id, now_ms],
        )?;
        tx.execute(
            "UPDATE sessions SET revision = ?2 WHERE id = ?1",
            params![session_id, cursor],
        )?;
        tx.commit()?;
        Ok(RaisedAttention {
            change: Change {
                cursor,
                session_ids: vec![session_id.clone()],
                attention_ids: vec![attention_id.clone()],
            },
            intent: NotificationIntent {
                request_id,
                attention_id,
                session_id,
                title: format!("Qualification: {label}"),
                body: summary,
            },
        })
    }

    fn synthetic_namespace(
        tx: &rusqlite::Transaction<'_>,
        endpoint_id: &str,
    ) -> Result<String, JournalError> {
        if let Some(id) = tx
            .query_row(
                "SELECT id FROM provider_namespaces WHERE provider = ?1 AND endpoint_id = ?2 AND profile_ref = ?3",
                params![FIXTURE_PROVIDER, endpoint_id, SYNTHETIC_PROFILE],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok(id);
        }
        let id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO provider_namespaces (id, provider, endpoint_id, profile_ref) VALUES (?1, ?2, ?3, ?4)",
            params![id, FIXTURE_PROVIDER, endpoint_id, SYNTHETIC_PROFILE],
        )?;
        Ok(id)
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
        let endpoint_id = self.endpoint_id.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let namespace_id = Self::synthetic_namespace(&tx, &endpoint_id)?;
        let payload = serde_json::json!({ "runId": run_id, "slot": slot, "sequence": sequence });
        let (_, cursor) = Self::insert_observation(
            &tx,
            SOURCE_QUALIFICATION,
            &self.source_epoch,
            "QUALIFY_SYNTHETIC_CHANGE",
            &payload,
            now_ms,
        )?;
        let native = format!("m0c-stream-{slot}");
        let name = format!("Stream {slot} · change {sequence} · run {run_id}");
        let new_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO sessions (id, namespace_id, native_session_id, record_state, display_name, fixture, revision)
             VALUES (?1, ?2, ?3, 'KNOWN', ?4, 1, ?5)
             ON CONFLICT(namespace_id, native_session_id)
               DO UPDATE SET display_name = excluded.display_name, revision = excluded.revision",
            params![new_id, namespace_id, native, name, cursor],
        )?;
        let session_id: String = tx.query_row(
            "SELECT id FROM sessions WHERE namespace_id = ?1 AND native_session_id = ?2",
            params![namespace_id, native],
            |row| row.get(0),
        )?;
        tx.commit()?;
        Ok(Change {
            cursor,
            session_ids: vec![session_id],
            attention_ids: Vec::new(),
        })
    }

    /// Adds `count` synthetic fixture Sessions with `name_bytes`-long names in
    /// one transaction (oversized-snapshot fixtures).
    pub fn populate_synthetic(
        &mut self,
        count: u32,
        name_bytes: u32,
        now_ms: i64,
    ) -> Result<Change, JournalError> {
        let endpoint_id = self.endpoint_id.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let namespace_id = Self::synthetic_namespace(&tx, &endpoint_id)?;
        let payload = serde_json::json!({ "count": count, "nameBytes": name_bytes });
        let (_, cursor) = Self::insert_observation(
            &tx,
            SOURCE_QUALIFICATION,
            &self.source_epoch,
            "QUALIFY_POPULATED",
            &payload,
            now_ms,
        )?;
        let padding = "x".repeat(name_bytes as usize);
        let mut session_ids = Vec::with_capacity(count as usize);
        for index in 0..count {
            let id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO sessions (id, namespace_id, native_session_id, record_state, display_name, fixture, revision)
                 VALUES (?1, ?2, ?3, 'KNOWN', ?4, 1, ?5)",
                params![id, namespace_id, format!("m0c-populate-{cursor}-{index}"), format!("Populated {index} {padding}"), cursor],
            )?;
            session_ids.push(id);
        }
        tx.commit()?;
        Ok(Change {
            cursor,
            session_ids,
            attention_ids: Vec::new(),
        })
    }
}
