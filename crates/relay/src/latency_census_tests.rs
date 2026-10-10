use super::*;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

const RUNTIME: &str = "11111111-1111-4111-8111-111111111111";
const CENSUS: &str = "22222222-2222-4222-8222-222222222222";
const BOOT: &str = "fixture-boot";

struct Fixture {
    store: std::path::PathBuf,
    anchor: String,
}
impl Fixture {
    fn new() -> Self {
        let store =
            std::env::temp_dir().join(format!("threadspace-census-test-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&store)
            .expect("acquire disposable store");
        let mut fixture = Self {
            store,
            anchor: String::new(),
        };
        fixture.anchor = fixture.add_anchor(RUNTIME);
        fixture
    }
    fn add_anchor(&self, runtime: &str) -> String {
        let receipt: super::super::ModBatchReceipt = serde_json::from_value(json!({ "receiptVersion": 1,
            "results": [{ "observationId": "33333333-3333-4333-8333-000000000000", "status": "COMMITTED", "reason": null }] })).expect("receipt");
        let request = serde_json::to_vec(&json!({ "sourceEpoch": runtime,
            "records": [{ "observationId": "33333333-3333-4333-8333-000000000000" }] }))
        .expect("request");
        let response = super::super::annotate_receipt(&self.store, &receipt, &request, BOOT, 10)
            .expect("actual independent native receipt fixture");
        serde_json::from_slice::<Value>(&response).expect("JSON")["qualificationClock"]["token"]
            .as_str()
            .expect("token")
            .to_owned()
    }
    fn records(&self) -> Vec<Value> {
        fs::read_dir(self.store.join("qualification/m2-latency"))
            .expect("collector")
            .map(|entry| {
                serde_json::from_slice(&fs::read(entry.expect("entry").path()).expect("metadata"))
                    .expect("JSON")
            })
            .collect()
    }
    fn send(&self, value: &Value, at: u64) -> Option<Value> {
        answer_census(
            &self.store,
            &serde_json::to_vec(value).expect("request"),
            BOOT,
            at,
            || Some(at),
        )
        .map(|bytes| serde_json::from_slice(&bytes).expect("receipt"))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.store);
    }
}

fn requests(fixture: &Fixture, count: usize) -> (Vec<Value>, Value) {
    let captures: Vec<Capture> = (0..count)
        .map(|index| Capture {
            observation_id: format!("33333333-3333-4333-8333-{index:012x}"),
            captured_ms: Some(1.0 + index as f64 / 8192.0),
        })
        .collect();
    let ids = captures
        .iter()
        .map(|capture| capture.observation_id.clone())
        .collect();
    let page_count = count.div_ceil(MAX_RECORDS);
    let pages = captures
        .chunks(MAX_RECORDS)
        .enumerate()
        .map(|(page_index, records)| {
            json!(Page {
                kind: "observer-census-page".into(),
                schema_version: 1,
                runtime_id: RUNTIME.into(),
                census_id: CENSUS.into(),
                page_index,
                page_count,
                records: records.to_vec(),
            })
        })
        .collect();
    let close = json!(Close {
        kind: "observer-census-close".into(),
        schema_version: 1,
        runtime_id: RUNTIME.into(),
        census_id: CENSUS.into(),
        page_count,
        boundary: "SESSION_END".into(),
        clock: "performance.now".into(),
        opened_ms: 1.0,
        closed_ms: 10.0,
        total_captured: count as u64,
        overflow: 0,
        failed_exports: 0,
        observation_ids_sha256: digest(&ids),
        end_drain_deadline: EndDrainDeadline {
            receipt_token: fixture.anchor.clone(),
            remaining_ms: 0.0001
        },
    });
    (pages, close)
}

fn persist_close(fixture: &Fixture, pages: &[Value], close: &Value) -> Value {
    for page in pages {
        let response = fixture.send(page, 20).expect("page stored");
        assert_eq!(response["status"], "PAGE_RECORDED");
        assert_eq!(response["pageIndex"], page["pageIndex"]);
    }
    fixture.send(close, 30).expect("complete ledger closed")
}
fn confirmation(closed: &Value) -> Value {
    json!({ "kind": "observer-census-confirm", "schemaVersion": 1, "runtimeId": RUNTIME, "censusId": CENSUS,
        "token": closed["token"], "observationIdsSha256": closed["observationIdsSha256"] })
}
fn seal(records: &[Value]) -> Result<Value, String> {
    observer_population_seal(records, &BTreeSet::from([RUNTIME.to_owned()]), BOOT, 1, 100)
}

fn retain(name: &str, record: Value) {
    let Some(directory) = std::env::var_os("THREADSPACE_CENSUS_EVIDENCE_DIR") else {
        return;
    };
    let path = std::path::PathBuf::from(directory).join(name);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("new owned evidence fixture");
    file.write_all(&serde_json::to_vec_pretty(&record).expect("fixture JSON"))
        .expect("retain fixture");
}

#[test]
fn complete_paged_ledger_requires_positive_close_receipt_confirmation() {
    let fixture = Fixture::new();
    let (pages, close) = requests(&fixture, 129);
    let closed = persist_close(&fixture, &pages, &close);
    assert_eq!(closed["status"], "CLOSED");
    let missing = seal(&fixture.records()).expect_err("unconfirmed close");
    assert!(missing.contains("not confirmed"));
    retain(
        "rust-close-without-confirmation.json",
        json!({ "executionKind": "PORTABLE_SYNTHETIC_CLOCKS", "nativeExecution": false,
        "helperRecords": fixture.records(), "verifierError": missing }),
    );
    let confirmed = fixture
        .send(&confirmation(&closed), 40)
        .expect("confirmation stored");
    assert_eq!(confirmed["status"], "CONFIRMED");
    assert_eq!(confirmed["token"], closed["token"]);
    let sealed = seal(&fixture.records()).expect("verified source census");
    assert_eq!(sealed["totalCaptured"], 129);
    assert_eq!(
        sealed["observationIdsSha256"],
        close["observationIdsSha256"]
    );
    assert_eq!(sealed["telemetryFailures"], 0);
    assert_eq!(sealed["runtimeIds"], json!([RUNTIME]));
    retain(
        "rust-complete-census.json",
        json!({ "executionKind": "PORTABLE_SYNTHETIC_CLOCKS", "nativeExecution": false,
        "helperRecords": fixture.records(), "populationSeal": sealed }),
    );
    for entry in fs::read_dir(fixture.store.join("qualification/m2-latency")).expect("collector") {
        assert_eq!(
            entry
                .expect("entry")
                .metadata()
                .expect("mode")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn missing_final_page_close_or_confirmation_cannot_be_replaced_by_prefix() {
    let fixture = Fixture::new();
    let (pages, close) = requests(&fixture, 129);
    fixture.send(&pages[0], 20).expect("first page");
    assert!(
        fixture.send(&close, 30).is_none(),
        "missing final page must refuse close"
    );
    assert!(seal(&fixture.records()).is_err());
    fixture.send(&pages[1], 21).expect("last page");
    assert!(
        seal(&fixture.records()).is_err(),
        "complete pages alone are not a close"
    );
    fixture.send(&close, 30).expect("close");
    assert!(
        seal(&fixture.records()).is_err(),
        "unheard close ACK is not a confirmed census"
    );
}

#[test]
fn changed_count_digest_order_duplicate_or_overflow_refuses_native_close() {
    for mutation in ["count", "digest", "duplicate", "order", "overflow"] {
        let fixture = Fixture::new();
        let (mut pages, mut close) = requests(&fixture, 2);
        match mutation {
            "count" => close["totalCaptured"] = json!(1),
            "digest" => close["observationIdsSha256"] = json!("0".repeat(64)),
            "duplicate" => pages[0]["records"][1] = pages[0]["records"][0].clone(),
            "order" => pages[0]["records"]
                .as_array_mut()
                .expect("records")
                .reverse(),
            "overflow" => close["overflow"] = json!(1),
            _ => unreachable!(),
        }
        let _ = fixture.send(&pages[0], 20);
        assert!(fixture.send(&close, 30).is_none(), "{mutation}");
    }
}

#[test]
fn stable_retries_are_idempotent_and_conflicting_page_never_overwrites() {
    let fixture = Fixture::new();
    let (pages, close) = requests(&fixture, 2);
    let closed = persist_close(&fixture, &pages, &close);
    let confirmed = confirmation(&closed);
    fixture.send(&confirmed, 40).expect("confirmation");
    let before = fixture.records().len();
    assert_eq!(fixture.send(&close, 50).expect("idempotent close"), closed);
    fixture.send(&pages[0], 51).expect("idempotent page");
    fixture
        .send(&confirmed, 52)
        .expect("idempotent confirmation");
    assert_eq!(before, fixture.records().len());
    let mut changed = pages[0].clone();
    changed["records"][0]["capturedMs"] = json!(2.0);
    assert!(fixture.send(&changed, 60).is_none());
    assert!(
        seal(&fixture.records()).is_ok(),
        "conflicting retry did not replace evidence"
    );
}

#[test]
fn fabricated_close_token_wrong_runtime_wrong_boot_or_wrong_store_cannot_confirm() {
    let fixture = Fixture::new();
    let (pages, close) = requests(&fixture, 2);
    let closed = persist_close(&fixture, &pages, &close);
    let good = confirmation(&closed);
    for key in ["token", "runtimeId", "censusId"] {
        let mut changed = good.clone();
        changed[key] = json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
        assert!(fixture.send(&changed, 40).is_none(), "{key}");
    }
    let bytes = serde_json::to_vec(&good).expect("request");
    assert!(answer_census(&fixture.store, &bytes, "other-boot", 40, || Some(40)).is_none());
    let other = Fixture::new();
    assert!(other.send(&good, 40).is_none());
    assert!(seal(&fixture.records()).is_err());
}

#[test]
fn native_submitted_uuid_absent_from_source_census_is_an_explicit_missing_tail() {
    let fixture = Fixture::new();
    let (pages, close) = requests(&fixture, 2);
    let closed = persist_close(&fixture, &pages, &close);
    fixture
        .send(&confirmation(&closed), 40)
        .expect("confirmation");
    let mut records = fixture.records();
    records.push(
        json!({ "kind": "helper-receipt", "runtimeId": RUNTIME, "bootId": BOOT,
        "monotonicNs": "25", "requestObservationIds": ["99999999-9999-4999-8999-999999999999"] }),
    );
    let error = seal(&records).expect_err("missing tail");
    assert!(error.contains("UUID absent"));
    retain(
        "rust-native-extra-tail.json",
        json!({ "executionKind": "PORTABLE_SYNTHETIC_CLOCKS", "nativeExecution": false,
        "injectedCounterexample": "additional native submitted UUID missing from closed source ledger", "helperRecords": records, "verifierError": error }),
    );
}

#[test]
fn every_seen_runtime_must_close_and_reused_uuid_between_runtimes_is_refused() {
    let fixture = Fixture::new();
    let (pages, close) = requests(&fixture, 2);
    let closed = persist_close(&fixture, &pages, &close);
    fixture
        .send(&confirmation(&closed), 40)
        .expect("confirmation");
    let second = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    let epochs = BTreeSet::from([RUNTIME.to_owned(), second.to_owned()]);
    assert!(observer_population_seal(&fixture.records(), &epochs, BOOT, 1, 100).is_err());
    let mut second_pages = pages;
    second_pages[0]["runtimeId"] = json!(second);
    let mut second_close = close;
    second_close["runtimeId"] = json!(second);
    second_close["endDrainDeadline"]["receiptToken"] = json!(fixture.add_anchor(second));
    let second_closed = persist_close(&fixture, &second_pages, &second_close);
    let mut confirm = confirmation(&second_closed);
    confirm["runtimeId"] = json!(second);
    fixture.send(&confirm, 40).expect("second confirmation");
    assert!(
        observer_population_seal(&fixture.records(), &epochs, BOOT, 1, 100)
            .expect_err("UUID reused")
            .contains("multiple source runtimes")
    );
}

#[test]
fn historical_export_failures_and_missing_capture_clocks_remain_in_final_seal() {
    let fixture = Fixture::new();
    let (mut pages, mut close) = requests(&fixture, 2);
    pages[0]["records"][0]["capturedMs"] = Value::Null;
    close["failedExports"] = json!(3);
    let closed = persist_close(&fixture, &pages, &close);
    fixture
        .send(&confirmation(&closed), 40)
        .expect("confirmation");
    let sealed = seal(&fixture.records()).expect("complete but unqualified ledger");
    assert_eq!(sealed["totalCaptured"], 2);
    assert_eq!(
        sealed["telemetryFailures"], 4,
        "a complete ledger cannot erase known missing telemetry"
    );
}

#[test]
fn boot_time_and_postclose_identity_tampering_refuse_reexport() {
    let fixture = Fixture::new();
    let (pages, close) = requests(&fixture, 2);
    let closed = persist_close(&fixture, &pages, &close);
    fixture
        .send(&confirmation(&closed), 40)
        .expect("confirmation");
    for (kind, key, value) in [
        (
            "observer-census-confirmed",
            "token",
            json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
        ),
        ("observer-census-confirmed", "monotonicNs", json!("29")),
        ("observer-census-closed", "bootId", json!("other-boot")),
        ("observer-census-page-record", "monotonicNs", json!("101")),
    ] {
        let mut records = fixture.records();
        records
            .iter_mut()
            .find(|record| record["kind"] == kind)
            .expect("target")[key] = value;
        assert!(seal(&records).is_err(), "{kind}.{key}");
    }
    assert!(
        observer_population_seal(
            &fixture.records(),
            &BTreeSet::from([RUNTIME.to_owned()]),
            BOOT,
            21,
            100
        )
        .is_err(),
        "pre-window pages"
    );
}

#[test]
fn removing_or_duplicating_native_file_after_ack_never_preserves_qualification() {
    let fixture = Fixture::new();
    let (pages, close) = requests(&fixture, 2);
    let closed = persist_close(&fixture, &pages, &close);
    fixture
        .send(&confirmation(&closed), 40)
        .expect("confirmation");
    for kind in [
        "observer-census-page-record",
        "observer-census-closed",
        "observer-census-confirmed",
    ] {
        let records = fixture.records();
        let kept: Vec<_> = records
            .iter()
            .filter(|value| value["kind"] != kind)
            .cloned()
            .collect();
        assert!(seal(&kept).is_err(), "missing {kind}");
        let mut duplicated = records.clone();
        duplicated.push(
            records
                .iter()
                .find(|value| value["kind"] == kind)
                .expect("file")
                .clone(),
        );
        assert!(seal(&duplicated).is_err(), "duplicate {kind}");
    }
}

#[test]
fn protocol_is_bounded_and_metadata_only() {
    let fixture = Fixture::new();
    let (pages, close) = requests(&fixture, 2);
    for key in ["prompt", "result", "providerPayload"] {
        let mut bad = pages[0].clone();
        bad[key] = json!("not accepted");
        assert!(fixture.send(&bad, 20).is_none(), "{key}");
    }
    let mut huge = pages[0].clone();
    huge["pageCount"] = json!(MAX_PAGES + 1);
    assert!(fixture.send(&huge, 20).is_none());
    let mut huge = close;
    huge["totalCaptured"] = json!(MAX_CAPTURES + 1);
    assert!(fixture.send(&huge, 30).is_none());
    assert!(
        answer_census(&fixture.store, &vec![b' '; MAX_BYTES + 1], BOOT, 20, || {
            Some(20)
        })
        .is_none()
    );
}

#[test]
fn maximum_bounded_ledger_closes_with_all_4096_uuid_entries() {
    let fixture = Fixture::new();
    let (pages, close) = requests(&fixture, MAX_CAPTURES);
    assert_eq!(pages.len(), 32);
    let closed = persist_close(&fixture, &pages, &close);
    fixture
        .send(&confirmation(&closed), 40)
        .expect("confirmation");
    assert_eq!(
        seal(&fixture.records()).expect("full bound")["totalCaptured"],
        MAX_CAPTURES
    );
}

#[test]
fn actual_typed_capture_receipt_is_unchanged_and_native_batch_ids_are_metadata_only() {
    let fixture = Fixture::new();
    let receipt: super::super::ModBatchReceipt = serde_json::from_value(json!({ "receiptVersion": 1,
        "results": [{ "observationId": "33333333-3333-4333-8333-000000000000", "status": "COMMITTED", "reason": null }] })).expect("real receipt type");
    let request = serde_json::to_vec(&json!({ "kind": "mod-batch", "sourceEpoch": RUNTIME,
        "records": [{ "observationId": "33333333-3333-4333-8333-000000000000", "payload": "PRIVATE_PROVIDER_CONTENT_MUST_NOT_BE_COPIED" }] })).expect("request");
    let annotated = super::super::annotate_receipt(&fixture.store, &receipt, &request, BOOT, 25)
        .expect("metadata saved");
    assert_eq!(
        serde_json::from_slice::<super::super::ModBatchReceipt>(&annotated)
            .expect("same canonical receipt"),
        receipt
    );
    let records = fixture.records();
    assert_eq!(records.len(), 2);
    let records: Vec<_> = records
        .into_iter()
        .filter(|value| value["monotonicNs"] == "25")
        .collect();
    assert_eq!(records[0]["runtimeId"], RUNTIME);
    assert_eq!(
        records[0]["requestObservationIds"],
        json!(["33333333-3333-4333-8333-000000000000"])
    );
    assert_eq!(
        records[0]["receipt"],
        serde_json::to_value(&receipt).expect("unchanged receipt")
    );
    assert!(
        !serde_json::to_string(&records)
            .expect("metadata")
            .contains("PRIVATE_PROVIDER_CONTENT")
    );
    let missing = fixture.store.join("not-an-existing-owned-store");
    assert!(
        super::super::annotate_receipt(&missing, &receipt, &request, BOOT, 25).is_none(),
        "telemetry failure supplies no replacement capture receipt"
    );
    assert!(!missing.exists(), "telemetry cannot create another store");
}

#[test]
fn native_deadline_rejects_missing_anchor_and_late_durable_confirmation() {
    let fixture = Fixture::new();
    let (pages, close) = requests(&fixture, 2);
    for page in &pages {
        fixture.send(page, 20).expect("page");
    }
    let mut wrong = close.clone();
    wrong["endDrainDeadline"]["receiptToken"] = json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    assert!(
        fixture.send(&wrong, 30).is_none(),
        "intercepted or missing clock token cannot anchor a deadline"
    );
    let closed = fixture.send(&close, 30).expect("valid bounded close");
    let body = serde_json::to_vec(&confirmation(&closed)).expect("confirmation");
    let response = answer_census(&fixture.store, &body, BOOT, 40, || Some(111));
    assert!(
        response.is_none(),
        "native echo completed after the original cutoff"
    );
    let records = fixture.records();
    assert!(
        records
            .iter()
            .any(|value| value["kind"] == "observer-census-confirm-received"),
        "echo really persisted before the post-fsync clock check"
    );
    assert!(
        !records
            .iter()
            .any(|value| value["kind"] == "observer-census-confirmed"),
        "no qualifying completion witness after deadline"
    );
    assert!(seal(&records).is_err());
    retain(
        "rust-late-durable-confirmation.json",
        json!({ "executionKind": "PORTABLE_SYNTHETIC_CLOCKS", "nativeExecution": false,
        "helperRecords": records, "confirmationEntryNs": "40", "confirmationDurableNs": "111", "nativeCutoffNs": "110", "qualified": false }),
    );
}

#[test]
fn completion_witness_requires_the_exact_durable_echo_and_deadline_anchor() {
    let fixture = Fixture::new();
    let (pages, close) = requests(&fixture, 2);
    let closed = persist_close(&fixture, &pages, &close);
    fixture
        .send(&confirmation(&closed), 40)
        .expect("confirmation");
    let records = fixture.records();
    for kind in ["helper-receipt", "observer-census-confirm-received"] {
        let remaining: Vec<_> = records
            .iter()
            .filter(|value| value["kind"] != kind)
            .cloned()
            .collect();
        assert!(seal(&remaining).is_err(), "missing {kind}");
    }
    let mut tampered = records;
    tampered
        .iter_mut()
        .find(|value| value["kind"] == "observer-census-confirm-received")
        .expect("echo")["monotonicNs"] = json!("39");
    assert!(
        seal(&tampered).is_err(),
        "completion hash cannot conceal a changed native echo"
    );
}
