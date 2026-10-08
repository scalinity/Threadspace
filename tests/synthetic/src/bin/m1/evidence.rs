//! Evidence output: one directory per area under `evidence/M1/`, written
//! deterministically (sorted keys, trailing newline) so reruns diff cleanly.

use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

pub struct Area {
    pub dir: PathBuf,
}

impl Area {
    pub fn new(root: &Path, name: &str) -> Result<Self, String> {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(Self { dir })
    }

    pub fn json(&self, name: &str, value: &Value) -> Result<String, String> {
        let mut text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
        text.push('\n');
        std::fs::write(self.dir.join(name), &text).map_err(|e| e.to_string())?;
        Ok(sha256(text.as_bytes()))
    }

    pub fn text(&self, name: &str, text: &str) -> Result<String, String> {
        std::fs::write(self.dir.join(name), text).map_err(|e| e.to_string())?;
        Ok(sha256(text.as_bytes()))
    }
}

pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_file(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|bytes| sha256(&bytes))
}

/// Order statistics of a sample in microseconds.
pub fn percentiles(samples: &mut [u64]) -> Value {
    samples.sort_unstable();
    let at = |p: f64| -> u64 {
        if samples.is_empty() {
            return 0;
        }
        let rank = ((p / 100.0) * samples.len() as f64).ceil() as usize;
        samples[rank.clamp(1, samples.len()) - 1]
    };
    serde_json::json!({
        "count": samples.len(),
        "p50Us": at(50.0),
        "p95Us": at(95.0),
        "p99Us": at(99.0),
        "maxUs": samples.last().copied().unwrap_or(0),
        "minUs": samples.first().copied().unwrap_or(0),
    })
}
