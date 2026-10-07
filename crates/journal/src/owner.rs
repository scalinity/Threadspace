//! Owner commands and preferences beyond acknowledgement (SPEC §7.2, §19.5).

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use threadspace_contracts::canonical::command::{OwnerAction, OwnerCommand};
use threadspace_contracts::cursor::format_cursor;

use crate::{CommandOutcome, Journal, JournalError, SOURCE_OWNER};

const OBSERVATION_ENABLED_KEY: &str = "observation_enabled";
const MAINTENANCE_PHASE_KEY: &str = "maintenance_phase";
const MAINTENANCE_PURPOSE_KEY: &str = "maintenance_purpose";
const SOURCE_COMPANION: &str = "companion.lifecycle";

impl Journal {
    /// "Mark handled": explicit owner resolution with a recorded reason. A
    /// repeated command ID with the same payload returns its receipt;
    /// conflicting reuse is rejected (SPEC §18.3). Resolving an already
    /// resolved item records the command and keeps the item resolved.
    pub fn resolve_attention(
        &mut self,
        command_id: &str,
        attention_id: &str,
        expected_revision: Option<i64>,
        reason: &str,
        now_ms: i64,
    ) -> Result<CommandOutcome, JournalError> {
        self.admit_owner_command(
            &OwnerCommand {
                command_id: command_id.to_owned(),
                attention_id: attention_id.to_owned(),
                expected_revision: expected_revision.map(format_cursor),
                action: OwnerAction::Resolve {
                    reason: reason.to_owned(),
                },
            },
            now_ms,
        )
    }

    /// Snoozes an item until `until_ms`: presentation and notification
    /// eligibility only, never provider state (SPEC §7.2).
    pub fn snooze_attention(
        &mut self,
        command_id: &str,
        attention_id: &str,
        expected_revision: Option<i64>,
        until_ms: i64,
        now_ms: i64,
    ) -> Result<CommandOutcome, JournalError> {
        self.admit_owner_command(
            &OwnerCommand {
                command_id: command_id.to_owned(),
                attention_id: attention_id.to_owned(),
                expected_revision: expected_revision.map(format_cursor),
                action: OwnerAction::Snooze { until_ms },
            },
            now_ms,
        )
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
