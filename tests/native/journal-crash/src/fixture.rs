//! Deterministic fixture records, flag parsing and a seeded PRNG shared by
//! the parent runner and its workers.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use threadspace_journal::ObservationAdmission;

pub const SOURCE: &str = "qualification.journal-crash";
pub const DB_NAME: &str = "journal.sqlite3";
const EVENT: &str = "QUALIFY_JOURNAL_CRASH";
const CAPTURED_BASE_MS: i64 = 1_790_000_000_000;

/// One fixture record. Its UUID and content depend only on the case name,
/// run and index, so a retry after restart re-delivers the identical record.
pub struct Fixture {
    pub id: String,
    epoch: String,
    payload: Value,
    captured: i64,
}

impl Fixture {
    pub fn new(case: &str, run: u16, index: u64) -> Self {
        Self::padded(case, run, index, 0)
    }

    /// With `pad` > 0 the payload carries that many filler bytes, enough to
    /// make SQLite spill an open transaction's pages to the WAL.
    pub fn padded(case: &str, run: u16, index: u64, pad: usize) -> Self {
        let code = case_code(case);
        let mut payload = json!({ "case": case, "run": run, "index": index });
        if pad > 0 {
            payload["pad"] = json!("p".repeat(pad));
        }
        Self {
            id: format!("{code:08x}-{run:04x}-4000-8000-{index:012x}"),
            epoch: format!("{case}-{run}"),
            payload,
            captured: CAPTURED_BASE_MS + i64::try_from(index).unwrap_or(i64::MAX),
        }
    }

    pub fn admission(&self) -> ObservationAdmission<'_> {
        ObservationAdmission {
            observation_id: &self.id,
            source_id: SOURCE,
            source_epoch: &self.epoch,
            source_sequence: None,
            native_event: EVENT,
            captured_wall_ms: self.captured,
            payload: &self.payload,
        }
    }
}

/// FNV-1a of the case name: a stable per-case UUID prefix.
fn case_code(case: &str) -> u32 {
    case.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

pub fn side(db: &Path, suffix: &str) -> PathBuf {
    let mut text = db.as_os_str().to_owned();
    text.push(suffix);
    PathBuf::from(text)
}

/// `--name value` options and bare `--switch` flags.
pub struct Flags {
    values: HashMap<String, String>,
    switches: HashSet<String>,
}

impl Flags {
    pub fn parse(args: &[String], switches: &[&str]) -> Result<Self, String> {
        let mut values = HashMap::new();
        let mut set = HashSet::new();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            let Some(name) = arg.strip_prefix("--") else {
                return Err(format!("unexpected argument {arg:?}"));
            };
            if switches.contains(&name) {
                set.insert(name.to_owned());
            } else {
                let value = iter
                    .next()
                    .ok_or_else(|| format!("--{name} needs a value"))?;
                values.insert(name.to_owned(), value.clone());
            }
        }
        Ok(Self {
            values,
            switches: set,
        })
    }

    pub fn get<T: FromStr>(&self, name: &str, default: Option<T>) -> Result<T, String> {
        match self.values.get(name) {
            Some(text) => text
                .parse()
                .map_err(|_| format!("--{name} {text:?} is not valid")),
            None => default.ok_or_else(|| format!("--{name} is required")),
        }
    }

    pub fn switch(&self, name: &str) -> bool {
        self.switches.contains(name)
    }
}

/// SplitMix64: reproducible record counts and crash positions from `--seed`.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `low..=high`.
    pub fn range(&mut self, low: u64, high: u64) -> u64 {
        low + self.next() % (high - low + 1)
    }
}
