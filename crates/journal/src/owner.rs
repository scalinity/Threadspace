//! Owner commands and preferences beyond acknowledgement (SPEC §7.2, §19.5).

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use threadspace_contracts::cursor::format_cursor;
use threadspace_contracts::ui::{CommandReceipt, ReceiptStatus};

use crate::{Change, CommandOutcome, Journal, JournalError, SOURCE_OWNER, fingerprint};

const OBSERVATION_ENABLED_KEY: &str = "observation_enabled";
const MAINTENANCE_PHASE_KEY: &str = "maintenance_phase";
const MAINTENANCE_PURPOSE_KEY: &str = "maintenance_purpose";
const SOURCE_COMPANION: &str = "companion.lifecycle";

impl Journal {
    /// "Mark handled": explicit owner resolution with a recorded reason. A
    /// repeated command ID with the same payload returns its receipt;
    /// conflicting reuse is rejected (SPEC §18.3). Resolving an already
    /// resolved item records the command and leaves the first resolution.
    pub fn resolve_attention(
        &mut self,
        command_id: &str,
        attention_id: &str,
        expected_revision: Option<i64>,
        reason: &str,
        now_ms: i64,
    ) -> Result<CommandOutcome, JournalError> {
        let payload = serde_json::json!({
            "action": "ResolveAttention",
            "attentionId": attention_id,
            "expectedRevision": expected_revision.map(format_cursor),
            "reason": reason,
        });
        let print = fingerprint(&payload);
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(String, String)> = tx
            .query_row(
                "SELECT payload_fingerprint, result_json FROM attention_commands WHERE command_id = ?1",
                params![command_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((recorded_print, result_json)) = existing {
            if recorded_print != print {
                return Err(JournalError::Conflict {
                    detail: format!(
                        "command {command_id} was already used for a different payload"
                    ),
                });
            }
            let mut receipt: CommandReceipt =
                serde_json::from_str(&result_json).map_err(|error| JournalError::Invalid {
                    detail: error.to_string(),
                })?;
            receipt.status = ReceiptStatus::AlreadyCommitted;
            return Ok(CommandOutcome {
                receipt,
                change: None,
            });
        }
        let current: Option<(String, i64)> = tx
            .query_row(
                "SELECT session_id, revision FROM attention_items WHERE id = ?1",
                params![attention_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((session_id, revision)) = current else {
            return Err(JournalError::NotFound {
                entity: "attention",
                id: attention_id.to_owned(),
            });
        };
        if let Some(expected) = expected_revision
            && expected != revision
        {
            return Err(JournalError::Conflict {
                detail: format!(
                    "attention {attention_id} is at revision {revision}, not {expected}"
                ),
            });
        }
        let (observation_id, cursor) = Self::insert_observation(
            &tx,
            SOURCE_OWNER,
            &self.source_epoch,
            "OWNER_COMMAND",
            &payload,
            now_ms,
        )?;
        tx.execute(
            "UPDATE attention_items
               SET resolved_at_ms = COALESCE(resolved_at_ms, ?2), revision = ?3
             WHERE id = ?1",
            params![attention_id, now_ms, cursor],
        )?;
        let receipt = CommandReceipt {
            command_id: command_id.to_owned(),
            status: ReceiptStatus::Committed,
            cursor: format_cursor(cursor),
            target_revision: format_cursor(cursor),
        };
        let result_json =
            serde_json::to_string(&receipt).map_err(|error| JournalError::Invalid {
                detail: error.to_string(),
            })?;
        tx.execute(
            "INSERT INTO attention_commands (command_id, attention_id, action, payload_json, payload_fingerprint,
               result_json, observation_id)
             VALUES (?1, ?2, 'ResolveAttention', ?3, ?4, ?5, ?6)",
            params![command_id, attention_id, payload.to_string(), print, result_json, observation_id],
        )?;
        tx.commit()?;
        Ok(CommandOutcome {
            receipt,
            change: Some(Change {
                cursor,
                session_ids: vec![session_id],
                attention_ids: vec![attention_id.to_owned()],
            }),
        })
    }

    /// The persisted observation preference; a store that never recorded
    /// one was created with observation enabled.
    pub fn observation_enabled(&self) -> Result<bool, JournalError> {
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM store_meta WHERE key = ?1",
                params![OBSERVATION_ENABLED_KEY],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value.as_deref() != Some("false"))
    }

    /// Durably records the owner's observation preference as a journaled
    /// settings change. Returns the commit cursor.
    pub fn set_observation_enabled(
        &mut self,
        enabled: bool,
        now_ms: i64,
    ) -> Result<i64, JournalError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let payload = serde_json::json!({ "setting": OBSERVATION_ENABLED_KEY, "enabled": enabled });
        let (_, cursor) = Self::insert_observation(
            &tx,
            SOURCE_OWNER,
            &self.source_epoch,
            "SETTINGS_CHANGED",
            &payload,
            now_ms,
        )?;
        tx.execute(
            "INSERT INTO store_meta (key, value) VALUES (?1, ?2)
               ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![
                OBSERVATION_ENABLED_KEY,
                if enabled { "true" } else { "false" }
            ],
        )?;
        tx.commit()?;
        Ok(cursor)
    }

    /// The durable maintenance phase and its purpose (JSON), if any (SPEC §19.5).
    pub fn maintenance_phase(&self) -> Result<(String, Option<String>), JournalError> {
        let read = |key: &str| -> Result<Option<String>, JournalError> {
            Ok(self
                .conn
                .query_row(
                    "SELECT value FROM store_meta WHERE key = ?1",
                    params![key],
                    |row| row.get(0),
                )
                .optional()?)
        };
        Ok((
            read(MAINTENANCE_PHASE_KEY)?.unwrap_or_else(|| "NONE".to_owned()),
            read(MAINTENANCE_PURPOSE_KEY)?,
        ))
    }

    /// Records a maintenance phase transition atomically with its journal
    /// observation. `purpose_json` is kept while a phase is active.
    pub fn record_maintenance_phase(
        &mut self,
        phase: &str,
        purpose_json: Option<&str>,
        detail: serde_json::Value,
        now_ms: i64,
    ) -> Result<i64, JournalError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let payload =
            serde_json::json!({ "phase": phase, "purpose": purpose_json, "detail": detail });
        let (_, cursor) = Self::insert_observation(
            &tx,
            SOURCE_COMPANION,
            &self.source_epoch,
            "MAINTENANCE_PHASE",
            &payload,
            now_ms,
        )?;
        tx.execute(
            "INSERT INTO store_meta (key, value) VALUES (?1, ?2)
               ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![MAINTENANCE_PHASE_KEY, phase],
        )?;
        match purpose_json {
            Some(purpose) => tx.execute(
                "INSERT INTO store_meta (key, value) VALUES (?1, ?2)
                   ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![MAINTENANCE_PURPOSE_KEY, purpose],
            )?,
            None => tx.execute(
                "DELETE FROM store_meta WHERE key = ?1",
                params![MAINTENANCE_PURPOSE_KEY],
            )?,
        };
        tx.commit()?;
        Ok(cursor)
    }

    /// Journals a companion lifecycle fact (sleep/wake, observer
    /// interruption) that changes no projection. Returns the commit cursor.
    pub fn record_lifecycle(
        &mut self,
        native_event: &str,
        payload: serde_json::Value,
        now_ms: i64,
    ) -> Result<i64, JournalError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (_, cursor) = Self::insert_observation(
            &tx,
            SOURCE_COMPANION,
            &self.source_epoch,
            native_event,
            &payload,
            now_ms,
        )?;
        tx.commit()?;
        Ok(cursor)
    }
}
