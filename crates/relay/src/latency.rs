//! Bounded, qualification-only clock evidence. This is never journal truth
//! or a capture ACK. The helper's native stamp is retained independently of
//! the interceptable process.run result; that result carries only a join key.

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use threadspace_contracts::canonical::capture::ModBatchReceipt;

const MAX_BYTES: usize = 64 * 1024;
const MAX_FILES: usize = 1024;
const MAX_RECORDS: usize = 128;

#[path = "latency_census.rs"]
mod census;
pub use census::{answer_census, observer_population_seal};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    observation_id: String,
    captured_ms: Option<f64>,
    status: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ObserverBracket {
    kind: String,
    schema_version: u32,
    runtime_id: String,
    clock: String,
    begin_ms: f64,
    end_ms: f64,
    native_token: Option<String>,
    total_captured: u64,
    overflow: u64,
    failed_exports: u64,
    records: Vec<Record>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    helper_phases: Option<serde_json::Value>,
}

fn uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

fn finite(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

fn validated(bytes: &[u8]) -> Option<ObserverBracket> {
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let value: ObserverBracket = serde_json::from_slice(bytes).ok()?;
    let mut ids = std::collections::BTreeSet::new();
    let good = value.kind == "observer-clock-bracket"
        && value.schema_version == 1
        && uuid(&value.runtime_id)
        && value.clock == "performance.now"
        && finite(value.begin_ms)
        && finite(value.end_ms)
        && value.begin_ms <= value.end_ms
        && value.native_token.as_deref().is_none_or(uuid)
        && !value.records.is_empty()
        && value.records.len() <= MAX_RECORDS
        && value.records.iter().all(|record| {
            uuid(&record.observation_id)
                && ids.insert(&record.observation_id)
                && record
                    .captured_ms
                    .is_none_or(|at| finite(at) && at <= value.begin_ms)
                && matches!(
                    record.status.as_str(),
                    "COMMITTED"
                        | "ALREADY_COMMITTED"
                        | "LOCAL_SPOOLED"
                        | "NOT_ACCEPTED"
                        | "UNKNOWN"
                )
        });
    good.then_some(value)
}

fn private_dir(path: &Path) -> io::Result<()> {
    match DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {
            if let Some(parent) = path.parent() {
                fs::File::open(parent)?.sync_all()?;
            }
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let meta = fs::symlink_metadata(path)?;
    // SAFETY: geteuid has no preconditions.
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unowned measurement directory",
        ));
    }
    Ok(())
}

fn measurement_dir(store: &Path) -> io::Result<PathBuf> {
    // The caller has already restricted this to the development agent's
    // store. Never create a missing store or follow an alternate store path.
    let meta = fs::symlink_metadata(store)?;
    // SAFETY: geteuid has no preconditions.
    if !meta.is_dir() || meta.file_type().is_symlink() || meta.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unowned store",
        ));
    }
    let root = store.join("qualification");
    private_dir(&root)?;
    let dir = root.join("m2-latency");
    private_dir(&dir)?;
    Ok(dir)
}

fn save_named(store: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_BYTES || name.contains('/') || name.contains('\\') {
        return Err(io::Error::other("invalid bounded measurement"));
    }
    let dir = measurement_dir(store)?;
    // A full collector refuses further measurements. Missing data then
    // fails population accounting; it is never silently rotated away.
    if fs::read_dir(&dir)?.take(MAX_FILES).count() >= MAX_FILES {
        return Err(io::Error::other("measurement collector full"));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join(name))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    // The positive metadata ACK follows both file and directory sync. A
    // crash before either completes leaves an incomplete census, not an ACK.
    fs::File::open(dir)?.sync_all()
}

fn save(store: &Path, kind: &str, bytes: &[u8]) -> io::Result<()> {
    save_named(
        store,
        &format!("{kind}-{}.json", uuid::Uuid::new_v4()),
        bytes,
    )
}

fn read_named(store: &Path, name: &str) -> io::Result<Option<Vec<u8>>> {
    if name.contains('/') || name.contains('\\') {
        return Err(io::Error::other("invalid measurement name"));
    }
    let dir = measurement_dir(store)?;
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join(name))
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let meta = file.metadata()?;
    // SAFETY: geteuid has no preconditions.
    if !meta.is_file()
        || meta.len() > MAX_BYTES as u64
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
    {
        return Err(io::Error::other("invalid private measurement file"));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        return Err(io::Error::other("measurement grew beyond bound"));
    }
    Ok(Some(bytes))
}

/// A bounded metadata-only report from the observer runtime. The receipt
/// returned to this exporter is deliberately unrelated to capture receipts.
pub fn save_observer(store: &Path, bytes: &[u8]) -> bool {
    let Some(value) = validated(bytes) else {
        return false;
    };
    serde_json::to_vec(&value)
        .ok()
        .is_some_and(|bytes| save(store, "observer", &bytes).is_ok())
}

/// A hook capture candidate with its original capture clock. Keeping this independent of
/// writer acceptance makes spool, rejection and missing commit telemetry
/// visible in the measurement denominator. Persistence is attempted only
/// after delivery/spool so measurement I/O cannot consume their budget.
/// A final source census is still mandatory: failure of both delivery and
/// this write cannot be detected from prior successful rows alone.
/// No provider payload is copied.
pub fn save_hook_capture(
    store: &Path,
    envelope: &threadspace_contracts::canonical::envelope::ObservationEnvelope,
) -> bool {
    let value = serde_json::json!({
        "kind": "hook-capture", "schemaVersion": 1,
        "observationId": envelope.observation_id,
        "source": envelope.source_id, "sourceEpoch": envelope.source_epoch, "capture": envelope.captured_at,
    });
    serde_json::to_vec(&value)
        .ok()
        .is_some_and(|bytes| save(store, "hook", &bytes).is_ok())
}

/// Records the actual native clock inside this helper invocation, after
/// the real receipt has been obtained. Measurement failure leaves that
/// receipt unchanged. Epochs and monotonic origins are not inferred here.
pub fn annotate_receipt(
    store: &Path,
    receipt: &ModBatchReceipt,
    request: &[u8],
    boot: &str,
    native_ns: u64,
) -> Option<Vec<u8>> {
    annotate_receipt_with_phases(store, receipt, request, boot, native_ns, None)
}

/// Optional helper phase stamps are retained beside the independent native
/// receipt token. They cannot replace capture, COMMIT or DOM clock evidence.
pub fn annotate_receipt_with_phases(
    store: &Path,
    receipt: &ModBatchReceipt,
    request: &[u8],
    boot: &str,
    native_ns: u64,
    phases: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<Vec<u8>> {
    // Independent metadata about the real submitted batch. Do not recover
    // the runtime population only from successful optional sample files.
    let request: serde_json::Value = serde_json::from_slice(request).ok()?;
    let runtime = request["sourceEpoch"]
        .as_str()
        .filter(|value| uuid(value))?;
    let records = request["records"]
        .as_array()
        .filter(|records| records.len() <= MAX_RECORDS)?;
    let ids: Vec<&str> = records
        .iter()
        .map(|record| record["observationId"].as_str().filter(|value| uuid(value)))
        .collect::<Option<_>>()?;
    let token = uuid::Uuid::new_v4().hyphenated().to_string();
    let native = serde_json::json!({
        "kind": "helper-receipt", "schemaVersion": 1, "token": token,
        "runtimeId": runtime, "requestObservationIds": ids,
        "clock": "CLOCK_UPTIME_RAW", "monotonicNs": native_ns.to_string(),
        "bootId": boot, "receipt": receipt,
        "helperPhaseStamps": phases,
    });
    save_named(
        store,
        &format!("native-{token}.json"),
        &serde_json::to_vec(&native).ok()?,
    )
    .ok()?;
    let mut response = serde_json::to_value(receipt).ok()?;
    response.as_object_mut()?.insert(
        "qualificationClock".into(),
        serde_json::json!({ "token": token }),
    );
    serde_json::to_vec(&response).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> serde_json::Value {
        serde_json::json!({
            "kind":"observer-clock-bracket", "schemaVersion":1,
            "runtimeId":"11111111-1111-4111-8111-111111111111", "clock":"performance.now",
            "beginMs":10.0,"endMs":20.0,"nativeToken":null,
            "totalCaptured":1,"overflow":0,"failedExports":0,
            "records":[{"observationId":"22222222-2222-4222-8222-222222222222", "capturedMs":9.0,"status":"UNKNOWN"}]
        })
    }
    #[test]
    fn metadata_validation_refuses_wall_clocks_bad_ids_future_capture_and_provider_content() {
        let bytes = |value: &serde_json::Value| serde_json::to_vec(value).expect("JSON");
        assert!(validated(&bytes(&sample())).is_some());
        for (key, wrong) in [
            ("clock", serde_json::json!("Date.now")),
            ("runtimeId", serde_json::json!("not-an-epoch")),
            ("endMs", serde_json::json!(1)),
        ] {
            let mut changed = sample();
            changed[key] = wrong;
            assert!(validated(&bytes(&changed)).is_none(), "{key}");
        }
        let mut changed = sample();
        changed["records"][0]["capturedMs"] = serde_json::json!(11);
        assert!(validated(&bytes(&changed)).is_none());
        let mut changed = sample();
        changed["prompt"] = serde_json::json!("not allowed");
        assert!(validated(&bytes(&changed)).is_none());
        let mut changed = sample();
        changed["records"]
            .as_array_mut()
            .expect("array")
            .push(sample()["records"][0].clone());
        assert!(validated(&bytes(&changed)).is_none());
        assert!(validated(&vec![b' '; MAX_BYTES + 1]).is_none());
    }
}
