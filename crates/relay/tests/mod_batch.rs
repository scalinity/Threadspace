//! `threadspace-hook mod-batch` prints its typed receipt for every batch,
//! including one far below a stdout buffer's size (the process ends with
//! `_exit`, which discards anything still buffered), and answers within the
//! budget the calling mod passes.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Instant;

use serde_json::json;
use threadspace_contracts::canonical::capture::{ModBatchReceipt, RecordStatus};

const EPOCH: &str = "6d1c7f0e-1111-4aaa-8bbb-000000000001";
const ID: &str = "00000000-0000-4000-8000-000000000001";

fn run(stdin: &[u8], budget_ms: u64) -> (std::process::Output, u128) {
    let store = std::env::temp_dir().join(format!("ts-mod-batch-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&store).expect("store");
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_threadspace-hook"))
        .args(["mod-batch", "--store-dir"])
        .arg(&store)
        .args(["--budget-ms", &budget_ms.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");
    child.stdin.take().expect("stdin").write_all(stdin).expect("write");
    let output = child.wait_with_output().expect("wait");
    let elapsed = started.elapsed().as_millis();
    let _ = std::fs::remove_dir_all(&store);
    (output, elapsed)
}

fn batch() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "receiptVersion": 1, "kind": "mod-batch", "sourceEpoch": EPOCH, "droppedRecords": 0,
        "records": [{
            "schemaVersion": 1, "observationId": ID, "adapterId": "threadspace-observer", "adapterVersion": "0.1.0",
            "sourceEpoch": EPOCH, "sequenceMeaning": "OBSERVER_CAPTURE",
            "callbackEntrySequence": "1", "callbackResultSequence": "2",
            "phase": "result", "nativeEvent": "turn.start",
            "dispatchOrigin": { "plugin": "engine", "tier": "core" }, "engineDispatch": true,
            "sessionId": "session-1", "sessionIdSource": "classic.SessionStart", "sessionGeneration": 1,
            "nativeTurnId": "turn-1", "payload": { "echoedTurnId": "turn-1" },
        }],
    }))
    .expect("json")
}

#[test]
fn a_batch_without_a_companion_is_spooled_and_answered_within_its_budget() {
    let (output, elapsed) = run(&batch(), 80);
    assert!(output.status.success());
    let receipt: ModBatchReceipt = serde_json::from_slice(&output.stdout).expect("a typed receipt on stdout");
    assert_eq!(receipt.results.len(), 1);
    assert_eq!(receipt.results[0].observation_id, ID);
    assert_eq!(receipt.results[0].status, RecordStatus::LocalSpooled);
    assert!(elapsed < 250, "answered in {elapsed} ms");
}

#[test]
fn input_that_is_not_a_mod_batch_claims_nothing() {
    for stdin in [b"[]".as_slice(), b"not json", br#"{"receiptVersion":2,"kind":"mod-batch","sourceEpoch":"x","droppedRecords":0,"records":[]}"#] {
        let (output, _) = run(stdin, 230);
        assert!(output.status.success());
        let receipt: ModBatchReceipt = serde_json::from_slice(&output.stdout).expect("a typed receipt");
        assert!(receipt.results.is_empty(), "{}", String::from_utf8_lossy(stdin));
    }
}
