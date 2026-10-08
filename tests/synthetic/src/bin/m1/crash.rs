//! Commit/ACK crash injection against canonical admission (MILESTONES M1:
//! one hundred injections; every durably acknowledged record survives).
//!
//! A worker process admits a fixed sequence of observations and owner
//! commands through the real journal, appending each receipt it received to
//! a synced acknowledgement file. The journal kills it (SIGKILL) at an armed
//! position of its N-th admission: before the transaction, after the
//! observation row, after facts and identity assignments, after reduction
//! and materialization, or after COMMIT before the receipt returns. The
//! harness then reopens the store as its only writer and checks every
//! acknowledged record and command, the store's internal consistency, and
//! that retrying the whole sequence converges with zero duplicate facts.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use threadspace_contracts::canonical::capture::RecordStatus;
use threadspace_contracts::canonical::command::{OwnerAction, OwnerCommand};
use threadspace_contracts::canonical::envelope::ObservationEnvelope;
use threadspace_contracts::canonical::fact::Delivery;
use threadspace_journal::{CRASH_AT_ENV, CRASH_POINT_ENV, CrashPoint, EnvelopeAdmission, Journal};
use threadspace_state_engine::hash::state_hash;
use threadspace_state_engine::ids::RandomAllocator;
use threadspace_state_engine::semantic::semantic_hash;
use threadspace_state_engine::synthetic::normalize_envelope;
use threadspace_synthetic::builder::{Step, Target};
use threadspace_synthetic::runner::target_attention;
use threadspace_synthetic::scenarios::{followup_claude, owner_commands, pid_tty_reuse, waiting};

use crate::evidence::Area;

const PER_POINT: u64 = 20;

/// The fixed admission sequence: observations from four scenarios with
/// distinct sessions, then owner commands on items they create.
fn sequence() -> (Vec<ObservationEnvelope>, Vec<(String, Target, OwnerAction)>) {
    let mut envelopes = Vec::new();
    let mut commands = Vec::new();
    for scenario in [followup_claude(), owner_commands(), pid_tty_reuse(), waiting()] {
        for step in scenario.steps {
            match step {
                Step::Observe(envelope) => envelopes.push(*envelope),
                Step::Owner(owner) => {
                    if !commands.iter().any(|(id, _, _)| *id == owner.command_id) {
                        commands.push((owner.command_id, owner.target, owner.action));
                    }
                }
            }
        }
    }
    (envelopes, commands)
}

fn open(store: &Path) -> Result<Journal, String> {
    Journal::open_with(
        &store.join("journal.sqlite3"),
        "crash-core",
        1,
        Box::new(RandomAllocator),
        false,
    )
    .map_err(|e| e.to_string())
}

fn append_ack(file: &mut std::fs::File, line: &str) -> Result<(), String> {
    file.write_all(format!("{line}\n").as_bytes()).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())
}

/// Admits the whole sequence; with `acks`, records every receipt received.
fn admit_all(journal: &mut Journal, acks: Option<&mut std::fs::File>) -> Result<Value, String> {
    let (envelopes, commands) = sequence();
    let mut acks = acks;
    let mut statuses = Vec::new();
    for (index, envelope) in envelopes.iter().enumerate() {
        let admission = EnvelopeAdmission {
            envelope,
            normalized: normalize_envelope(envelope),
        };
        let outcome = journal
            .admit_batch(&[admission], Delivery::Live, 1_000 + index as i64)
            .map_err(|e| e.to_string())?;
        let record = outcome.records.first().ok_or("no receipt")?;
        statuses.push(record.status);
        if let Some(file) = acks.as_deref_mut() {
            append_ack(file, &format!("obs {} {:?}", record.observation_id, record.status))?;
        }
    }
    let mut command_cursors = Vec::new();
    for (command_id, target, action) in &commands {
        let attention_id = target_attention(journal.canonical_state(), target).ok_or("command target missing")?;
        let outcome = journal
            .admit_owner_command(
                &OwnerCommand {
                    command_id: command_id.clone(),
                    attention_id,
                    expected_revision: None,
                    action: action.clone(),
                },
                5_000,
            )
            .map_err(|e| e.to_string())?;
        command_cursors.push(json!([command_id, outcome.receipt.cursor]));
        if let Some(file) = acks.as_deref_mut() {
            append_ack(file, &format!("cmd {command_id} {}", outcome.receipt.cursor))?;
        }
    }
    Ok(json!({
        "committed": statuses.iter().filter(|s| **s == RecordStatus::Committed).count(),
        "alreadyCommitted": statuses.iter().filter(|s| **s == RecordStatus::AlreadyCommitted).count(),
        "notAccepted": statuses.iter().filter(|s| **s == RecordStatus::NotAccepted).count(),
        "commands": command_cursors,
    }))
}

/// The worker: admits the sequence until the armed crash kills it.
pub fn worker(store: &Path, acks: &Path) -> Result<(), String> {
    let mut journal = open(store)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(acks)
        .map_err(|e| e.to_string())?;
    admit_all(&mut journal, Some(&mut file))?;
    Ok(())
}

pub fn matrix(root: &Path, self_exe: &Path) -> Result<Value, String> {
    let area = Area::new(root, "crash")?;
    // The clean reference: the same sequence with no crash.
    let reference_dir = scratch("reference");
    let mut reference = open(&reference_dir)?;
    admit_all(&mut reference, None)?;
    let reference_digest = reference.replay_digest().map_err(|e| e.to_string())?;
    let reference_semantic = semantic_hash(reference.canonical_state());
    drop(reference);
    let _ = std::fs::remove_dir_all(&reference_dir);

    let mut rows = Vec::new();
    let mut lost_total = 0usize;
    let mut duplicate_facts_total = 0i64;
    let mut pass = true;
    for point in CrashPoint::CANONICAL {
        for at in 1..=PER_POINT {
            let store = scratch(&format!("{}-{at}", point.name()));
            let acks = store.join("acks.log");
            let status = Command::new(self_exe)
                .args(["crash-worker", &store.display().to_string(), &acks.display().to_string()])
                .env(CRASH_POINT_ENV, point.name())
                .env(CRASH_AT_ENV, at.to_string())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map_err(|e| e.to_string())?;
            let killed = std::os::unix::process::ExitStatusExt::signal(&status) == Some(libc::SIGKILL);
            let acked: Vec<String> = std::fs::read_to_string(&acks)
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect();
            // Reopen as the only writer.
            let mut journal = open(&store)?;
            let entries = journal.journal_entries(0).map_err(|e| e.to_string())?;
            let committed: std::collections::BTreeSet<&str> =
                entries.iter().map(|e| e.observation_id.as_str()).collect();
            let lost: Vec<&String> = acked
                .iter()
                .filter(|line| {
                    line.strip_prefix("obs ")
                        .and_then(|rest| rest.split(' ').next())
                        .is_some_and(|id| !committed.contains(id))
                })
                .collect();
            let consistent = journal
                .replay_digest()
                .map(|d| d.projection_sha256 == d.tables_sha256 && d.state_sha256 == state_hash(journal.canonical_state()))
                .unwrap_or(false);
            // Retry everything: acknowledged records must come back
            // ALREADY_COMMITTED, the rest commit, and nothing duplicates.
            let retry = admit_all(&mut journal, None)?;
            // A second retry: every record ALREADY_COMMITTED and every owner
            // command resolving to the receipt the first retry committed.
            let again = admit_all(&mut journal, None)?;
            let commands_stable = again["commands"] == retry["commands"]
                && again["committed"] == 0
                && again["notAccepted"] == 0;
            let digest = journal.replay_digest().map_err(|e| e.to_string())?;
            let duplicate_facts = digest.facts as i64 - reference_digest.facts as i64;
            let converged = semantic_hash(journal.canonical_state()) == reference_semantic;
            let ok = killed
                && lost.is_empty()
                && consistent
                && duplicate_facts == 0
                && converged
                && commands_stable
                && digest.entries == reference_digest.entries;
            pass &= ok;
            lost_total += lost.len();
            duplicate_facts_total += duplicate_facts.abs();
            rows.push(json!({
                "point": point.name(),
                "atAdmission": at,
                "killedBySigkill": killed,
                "acknowledgedBeforeCrash": acked.len(),
                "acknowledgedLost": lost.len(),
                "storeConsistentAfterRestart": consistent,
                "retry": retry,
                "secondRetryStable": commands_stable,
                "entriesAfterRetry": digest.entries,
                "duplicateFacts": duplicate_facts,
                "convergedToReference": converged,
                "pass": ok,
            }));
            drop(journal);
            let _ = std::fs::remove_dir_all(&store);
        }
    }
    let summary = json!({
        "area": "crash",
        "pass": pass,
        "injections": rows.len(),
        "points": CrashPoint::CANONICAL.iter().map(|p| p.name()).collect::<Vec<_>>(),
        "perPoint": PER_POINT,
        "acknowledgedLost": lost_total,
        "duplicateFacts": duplicate_facts_total,
        "reference": { "entries": reference_digest.entries, "facts": reference_digest.facts },
        "durability": "SQLite WAL with synchronous=FULL on the local APFS volume; the worker is killed with SIGKILL (an ordinary process crash). Power-loss durability is not claimed.",
        "matrix": rows,
    });
    area.json("summary.json", &summary)?;
    Ok(summary)
}

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("threadspace-m1-crash-{label}-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}
