//! `threadspace-hook mod-batch` prints its typed receipt for every batch,
//! including one far below a stdout buffer's size: the process ends with
//! `_exit`, which discards anything still buffered.

use std::io::Write;
use std::process::{Command, Stdio};

use threadspace_contracts::canonical::capture::{ModBatchReceipt, RecordStatus};
use threadspace_relay::capture::{HookCapture, claude_hook_envelope, local_clock, sample_hook_input};

#[test]
fn a_one_record_batch_without_a_companion_prints_a_spooled_receipt() {
    let store = std::env::temp_dir().join(format!("ts-mod-batch-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&store).expect("store");
    let envelope = claude_hook_envelope(
        &sample_hook_input("Stop", "session-1"),
        HookCapture {
            observation_id: "00000000-0000-4000-8000-000000000001".into(),
            profile_ref: "claude-cli:~/.claude".into(),
            clock: local_clock(None, 1, None),
            evidence: Vec::new(),
        },
    )
    .expect("envelope");
    let mut child = Command::new(env!("CARGO_BIN_EXE_threadspace-hook"))
        .args(["mod-batch", "--store-dir"])
        .arg(&store)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(&serde_json::to_vec(&[&envelope]).expect("json"))
        .expect("write");
    let output = child.wait_with_output().expect("wait");

    assert!(output.status.success());
    let receipt: ModBatchReceipt = serde_json::from_slice(&output.stdout).expect("a typed receipt on stdout");
    assert_eq!(receipt.receipts.len(), 1);
    assert_eq!(receipt.receipts[0].observation_id, envelope.observation_id);
    assert_eq!(receipt.receipts[0].status, RecordStatus::LocalSpooled);
    let _ = std::fs::remove_dir_all(&store);
}
