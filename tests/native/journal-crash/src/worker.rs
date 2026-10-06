//! Worker processes the runner re-executes itself as. A worker is the real
//! single writer: it holds `WriterLock`, opens `Journal`, and prints each
//! receipt as a JSON line only after `admit_observation` returned it; that
//! printed line is the ACK the parent relies on.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::{Value, json};
use threadspace_journal::{Journal, LockError, WriterLock};

use crate::fixture::{DB_NAME, Fixture, Flags, now_ms};

/// Exit status of a worker refused because another writer holds the lock.
pub const EXIT_LOCK_HELD: u8 = 75;
const EXIT_FAILED: u8 = 70;

pub fn main(args: &[String]) -> ExitCode {
    let result = match args.first().map(String::as_str) {
        Some("admit") => admit(&args[1..]),
        Some("contend") => contend(&args[1..]),
        _ => Err("worker admit|contend".to_owned()),
    };
    result.unwrap_or_else(|detail| {
        emit(&json!({ "event": "error", "detail": detail }));
        ExitCode::from(EXIT_FAILED)
    })
}

fn emit(value: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

/// Admits fixture records `from..from+count`, ACKing each; with `--hold`,
/// then keeps the writer open until stdin closes or the parent kills it.
/// Crash points come from the environment the parent sets.
fn admit(args: &[String]) -> Result<ExitCode, String> {
    let flags = Flags::parse(args, &["hold"])?;
    let store: PathBuf = flags.get("store", None)?;
    let case: String = flags.get("case", None)?;
    let run: u16 = flags.get("run", None)?;
    let from: u64 = flags.get("from", Some(1))?;
    let count: u64 = flags.get("count", None)?;
    let pad: usize = flags.get("pad", Some(0))?;

    let lock = match WriterLock::acquire(&store) {
        Ok(lock) => lock,
        Err(LockError::Held { .. }) => {
            emit(&json!({ "event": "lock-held" }));
            return Ok(ExitCode::from(EXIT_LOCK_HELD));
        }
        Err(error) => return Err(error.to_string()),
    };
    let epoch = format!("worker-{}", std::process::id());
    let mut journal =
        Journal::open(&store.join(DB_NAME), &epoch, now_ms()).map_err(|error| error.to_string())?;
    emit(&json!({
        "event": "ready",
        "pid": std::process::id(),
        "cursor": journal.cursor().map_err(|error| error.to_string())?,
    }));
    for index in from..from + count {
        let fixture = Fixture::padded(&case, run, index, pad);
        let receipt = journal
            .admit_observation(&fixture.admission(), now_ms())
            .map_err(|error| format!("admit {index}: {error}"))?;
        emit(&json!({ "event": "ack", "index": index, "receipt": receipt }));
    }
    emit(&json!({ "event": "done" }));
    if flags.switch("hold") {
        let mut sink = Vec::new();
        let _ = std::io::stdin().read_to_end(&mut sink);
    }
    drop(journal);
    drop(lock);
    Ok(ExitCode::SUCCESS)
}

/// Tries to become a second writer.
fn contend(args: &[String]) -> Result<ExitCode, String> {
    let flags = Flags::parse(args, &[])?;
    let store: PathBuf = flags.get("store", None)?;
    match WriterLock::acquire(&store) {
        Err(LockError::Held { .. }) => {
            emit(&json!({ "event": "lock-held" }));
            Ok(ExitCode::from(EXIT_LOCK_HELD))
        }
        Ok(_lock) => {
            emit(&json!({ "event": "lock-acquired" }));
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => Err(error.to_string()),
    }
}
