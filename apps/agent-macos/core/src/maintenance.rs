//! The consistent pre-maintenance backup (SPEC §9.4, §19.5): SQLite's online
//! backup API into the store's `backups/` directory, verified by the journal
//! before the companion reports PREPARED. Never a copy of the live main file.

use std::path::Path;

use threadspace_journal::Journal;

pub struct BackupSummary {
    pub file_name: String,
    pub cursor: i64,
    pub sha256: String,
    pub bytes: u64,
}

pub fn consistent_backup(journal: &mut Journal, store_dir: &Path) -> Result<BackupSummary, String> {
    let dir = store_dir.join("backups");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let info = journal
        .backup_into(&dir, crate::log::now_ms())
        .map_err(|error| error.to_string())?;
    Ok(BackupSummary {
        file_name: info
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        cursor: info.cursor,
        sha256: info.sha256,
        bytes: info.bytes,
    })
}
