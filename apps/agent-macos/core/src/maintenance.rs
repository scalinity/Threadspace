//! The consistent pre-maintenance backup (SPEC §9.4, §19.5): SQLite's online
//! backup API into the store's `backups/` directory, verified before the
//! companion reports PREPARED. Never a copy of the live main file.

use std::path::Path;

use threadspace_journal::Journal;

pub struct BackupSummary {
    pub file_name: String,
    pub cursor: i64,
    pub sha256: String,
    pub bytes: u64,
}

pub fn consistent_backup(
    _journal: &mut Journal,
    _store_dir: &Path,
) -> Result<BackupSummary, String> {
    Err("consistent backup is not available in this build".into())
}
