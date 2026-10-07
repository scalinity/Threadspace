//! Structured JSON-lines log in `~/Library/Logs/<agent id>/agent.log`, rotated
//! at 10 MiB across five files (SPEC §19.4). Entries carry event codes and
//! internal IDs, never payload text.
//!
//! Framing: each record and its newline go to the append-mode file in one
//! `write_all` of one buffer, so a process killed between log calls never
//! leaves a record without its line end. One write narrows the window for a
//! torn record; it is not crash-atomic storage. A torn tail left by a killed
//! predecessor is ended on its own line when the log is opened, so it can
//! never join the next record; readers skip such a line.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde_json::{Map, Value};

const ROTATE_BYTES: u64 = 10 * 1024 * 1024;
const KEEP_FILES: u32 = 5;

static LOG: OnceLock<Mutex<File>> = OnceLock::new();

fn rotate(path: &Path) {
    if fs::metadata(path).map(|meta| meta.len()).unwrap_or(0) < ROTATE_BYTES {
        return;
    }
    for index in (1..KEEP_FILES).rev() {
        let from: PathBuf = path.with_extension(format!("log.{index}"));
        let to = path.with_extension(format!("log.{}", index + 1));
        let _ = fs::rename(from, to);
    }
    let _ = fs::rename(path, path.with_extension("log.1"));
}

pub fn init(log_dir: &Path) {
    let _ = fs::create_dir_all(log_dir);
    let path = log_dir.join("agent.log");
    rotate(&path);
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(&path)
    {
        if ends_mid_record(&file) {
            let _ = file.write_all(b"\n");
        }
        let _ = LOG.set(Mutex::new(file));
    }
}

/// Whether the file is non-empty and its last byte is not a newline: a
/// record torn by a process killed while writing it.
fn ends_mid_record(file: &File) -> bool {
    let len = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    let mut last = [0u8; 1];
    len > 0 && file.read_at(&mut last, len - 1).is_ok_and(|read| read == 1) && last[0] != b'\n'
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// Writes one event line. `fields` must be a JSON object of identifiers and codes.
pub fn event(level: &str, code: &str, fields: Value) {
    let mut line = Map::new();
    line.insert("ts".into(), Value::from(now_ms()));
    line.insert("pid".into(), Value::from(std::process::id()));
    line.insert("level".into(), Value::from(level));
    line.insert("event".into(), Value::from(code));
    if let Value::Object(extra) = fields {
        line.extend(extra);
    }
    let mut text = Value::Object(line).to_string();
    match LOG.get() {
        Some(file) => {
            // One buffer, one `write_all`: `writeln!` wrote the newline separately.
            text.push('\n');
            if let Ok(mut file) = file.lock() {
                let _ = file.write_all(text.as_bytes());
            }
        }
        None => eprintln!("{text}"),
    }
}

pub fn info(code: &str, fields: Value) {
    event("info", code, fields);
}

pub fn warn(code: &str, fields: Value) {
    event("warn", code, fields);
}

pub fn error(code: &str, fields: Value) {
    event("error", code, fields);
}
