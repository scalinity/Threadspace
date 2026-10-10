//! Replay a consistent private copy of the Dev journal. Never open the live
//! store with Journal and never export its unrelated canonical contents.

use std::time::Duration;

use serde_json::{Value, json};
use threadspace_harness::evidence::sha256_file;
use threadspace_harness::run::run;
use threadspace_journal::Journal;
use threadspace_state_engine::hash::state_hash;

use crate::ctx::Ctx;

pub fn qualify(ctx: &Ctx) -> Result<Value, String> {
    if ctx.channel_name() != "dev" {
        return Err("native replay is Dev-only".into());
    }
    let private = std::env::var_os("THREADSPACE_M2_PRIVATE_EVIDENCE")
        .map(std::path::PathBuf::from)
        .ok_or("explicit private evidence directory required")?;
    let meta = std::fs::symlink_metadata(&private).map_err(|e| e.to_string())?;
    if !private.is_absolute() || !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("private evidence must be an existing absolute directory".into());
    }
    let root = private.join(format!("native-replay-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).map_err(|e| e.to_string())?;
    let db = root.join("journal.sqlite3");
    // Python opens only the source read-only. The destination directory was
    // exclusively created above; SQLite's backup API includes the live WAL.
    let backup = run(
        "/usr/bin/python3",
        &[
            "-c",
            "import sqlite3,sys,pathlib; s=sqlite3.connect(pathlib.Path(sys.argv[1]).as_uri()+'?mode=ro',uri=True,timeout=2); d=sqlite3.connect(sys.argv[2]); s.backup(d); assert d.execute('PRAGMA integrity_check').fetchone()[0]=='ok'; d.close(); s.close()",
            &ctx.id.agent.journal.display().to_string(),
            &db.display().to_string(),
        ],
        Duration::from_secs(20),
    );
    if !backup.ok {
        return Err(format!(
            "read-only Dev backup failed; private copy retained: {}",
            backup.stderr
        ));
    }
    let backup_hash = sha256_file(&db);
    let mut journal = Journal::open(&db, "m2-native-copy-replay", threadspace_harness::now_ms())
        .map_err(|e| e.to_string())?;
    let recovered = state_hash(journal.canonical_state());
    let genesis = state_hash(&journal.replay_from_genesis().map_err(|e| e.to_string())?);
    let digest = journal.replay_digest().map_err(|e| e.to_string())?;
    let differences = journal
        .projection_differences()
        .map_err(|e| e.to_string())?;
    let checkpoint = journal
        .checkpoint("M2_PRIVATE_NATIVE_REPLAY", threadspace_harness::now_ms())
        .map_err(|e| e.to_string())?;
    drop(journal);
    let restart = Journal::open(&db, "m2-native-copy-restart", threadspace_harness::now_ms())
        .map_err(|e| e.to_string())?;
    let restarted = state_hash(restart.canonical_state());
    let pass = recovered == genesis
        && recovered == restarted
        && recovered == checkpoint
        && recovered == digest.state_sha256
        && digest.projection_sha256 == digest.tables_sha256
        && differences.is_empty();
    let value = json!({"sourceCommit":run("git", &["rev-parse","HEAD"], Duration::from_secs(3)).stdout.trim(),
        "harnessSha256":std::env::current_exe().ok().as_deref().and_then(sha256_file),
        "liveSourceOpenedReadOnly":true,"copy":root,"backupSha256":backup_hash,
        "backupIntegrity":"ok","recoveredStateSha256":recovered,"genesisStateSha256":genesis,
        "restartedStateSha256":restarted,"checkpointSha256":checkpoint,"digest":digest,
        "projectionDifferences":differences,"pass":pass});
    std::fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(value)
}
