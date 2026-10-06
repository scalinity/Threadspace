//! Idempotent admission of caller-identified observations (SPEC §8.4, G10).
//!
//! The caller names each observation with a UUID (the spool record's name).
//! One IMMEDIATE transaction writes it, and the receipt exists only after
//! COMMIT returns, so a receipt is a journal ACK. A repeat delivery of the
//! same UUID and content writes nothing and returns ALREADY_COMMITTED with
//! the original cursor; reusing a UUID for different content is rejected, as
//! for owner command IDs.

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use threadspace_contracts::ui::ReceiptStatus;
use uuid::Uuid;

#[cfg(feature = "qualification")]
use crate::crash::{self, CrashPoint};
use crate::{Journal, JournalError};

/// One observation offered for admission under its caller-supplied UUID.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservationAdmission<'a> {
    pub observation_id: &'a str,
    pub source_id: &'a str,
    pub source_epoch: &'a str,
    pub source_sequence: Option<&'a str>,
    pub native_event: &'a str,
    pub captured_wall_ms: i64,
    pub payload: &'a serde_json::Value,
}

/// Returned only after COMMIT; `cursor` is the record's journal position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdmissionReceipt {
    /// Canonical (lowercase, hyphenated) form of the admitted UUID.
    pub observation_id: String,
    pub status: ReceiptStatus,
    pub cursor: i64,
}

struct Recorded {
    cursor: i64,
    source_id: String,
    source_epoch: String,
    source_sequence: Option<String>,
    native_event: String,
    captured_wall_ms: i64,
    payload_json: String,
}

impl Journal {
    /// Commits `observation` once. A duplicate UUID with identical content
    /// returns ALREADY_COMMITTED and its original cursor without writing.
    pub fn admit_observation(
        &mut self,
        observation: &ObservationAdmission<'_>,
        now_ms: i64,
    ) -> Result<AdmissionReceipt, JournalError> {
        let observation_id = Uuid::parse_str(observation.observation_id)
            .map_err(|_| JournalError::Invalid {
                detail: format!(
                    "observation id {:?} is not a UUID",
                    observation.observation_id
                ),
            })?
            .hyphenated()
            .to_string();
        let payload_json = observation.payload.to_string();

        #[cfg(feature = "qualification")]
        let armed = self.crash.next_admission();
        #[cfg(feature = "qualification")]
        crash::hit(armed, CrashPoint::BeforeTransaction);

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let recorded = tx
            .query_row(
                "SELECT ingest_seq, source_id, source_epoch, source_sequence, native_event,
                        captured_wall_ms, payload_json
                   FROM observations WHERE observation_id = ?1",
                params![observation_id],
                |row| {
                    Ok(Recorded {
                        cursor: row.get(0)?,
                        source_id: row.get(1)?,
                        source_epoch: row.get(2)?,
                        source_sequence: row.get(3)?,
                        native_event: row.get(4)?,
                        captured_wall_ms: row.get(5)?,
                        payload_json: row.get(6)?,
                    })
                },
            )
            .optional()?;
        if let Some(recorded) = recorded {
            // Dropping `tx` rolls back the empty transaction: nothing is written.
            let same = recorded.source_id == observation.source_id
                && recorded.source_epoch == observation.source_epoch
                && recorded.source_sequence.as_deref() == observation.source_sequence
                && recorded.native_event == observation.native_event
                && recorded.captured_wall_ms == observation.captured_wall_ms
                && recorded.payload_json == payload_json;
            if !same {
                return Err(JournalError::Conflict {
                    detail: format!(
                        "observation {observation_id} was already admitted with different content"
                    ),
                });
            }
            return Ok(AdmissionReceipt {
                observation_id,
                status: ReceiptStatus::AlreadyCommitted,
                cursor: recorded.cursor,
            });
        }

        tx.execute(
            "INSERT INTO observations (observation_id, source_id, source_epoch, source_sequence,
               native_event, captured_wall_ms, received_wall_ms, payload_version, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8)",
            params![
                observation_id,
                observation.source_id,
                observation.source_epoch,
                observation.source_sequence,
                observation.native_event,
                observation.captured_wall_ms,
                now_ms,
                payload_json
            ],
        )?;
        let cursor = tx.last_insert_rowid();
        #[cfg(feature = "qualification")]
        crash::hit(armed, CrashPoint::InTransaction);
        tx.commit()?;
        #[cfg(feature = "qualification")]
        crash::hit(armed, CrashPoint::AfterCommitBeforeReceipt);

        Ok(AdmissionReceipt {
            observation_id,
            status: ReceiptStatus::Committed,
            cursor,
        })
    }

    /// Qualification only: every admitted `(observation_id, cursor)` from
    /// `source_id`, oldest first, for crash and backup census checks.
    #[cfg(feature = "qualification")]
    pub fn admitted_observations(
        &self,
        source_id: &str,
    ) -> Result<Vec<(String, i64)>, JournalError> {
        let mut statement = self.conn.prepare(
            "SELECT observation_id, ingest_seq FROM observations
              WHERE source_id = ?1 ORDER BY ingest_seq",
        )?;
        let rows = statement
            .query_map(params![source_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}
