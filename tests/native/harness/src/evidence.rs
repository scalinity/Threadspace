//! Evidence run directories under `evidence/M0C/<area>/<run>/`. Nothing is
//! overwritten: each run gets its own directory, JSONL is appended, and a
//! failed run stays beside a later passing one.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

pub struct Run {
    pub dir: PathBuf,
}

impl Run {
    /// `root/<area>/<YYYYMMDDTHHMMSSZ>-<label>/`.
    pub fn create(root: &Path, area: &str, label: &str) -> std::io::Result<Self> {
        let stamp = utc_stamp(crate::now_ms());
        let dir = root.join(area).join(format!("{stamp}-{label}"));
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    pub fn write_json(&self, name: &str, value: &Value) -> std::io::Result<PathBuf> {
        let path = self.dir.join(name);
        fs::write(&path, serde_json::to_vec_pretty(value).unwrap_or_default())?;
        Ok(path)
    }

    pub fn append(&self, name: &str, value: &Value) -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join(name))?;
        writeln!(file, "{value}")
    }

    pub fn write_text(&self, name: &str, text: &str) -> std::io::Result<PathBuf> {
        let path = self.dir.join(name);
        fs::write(&path, text)?;
        Ok(path)
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

pub fn sha256_file(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(hex(&Sha256::digest(&bytes)))
}

pub fn sha256_text(text: &str) -> String {
    hex(&Sha256::digest(text.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `YYYYMMDDTHHMMSSZ` for a Unix-epoch millisecond time (UTC).
pub fn utc_stamp(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z")
}

#[cfg(test)]
mod tests {
    #[test]
    fn utc_stamp_matches_a_known_instant() {
        // 2026-10-06T09:15:00Z
        assert_eq!(super::utc_stamp(1_791_278_100_000), "20261006T091500Z");
    }
}
