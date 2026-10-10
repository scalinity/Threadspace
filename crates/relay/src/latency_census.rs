//! The observer's qualification census protocol. This contains no journal
//! admission and its receipts are never capture receipts. The source sends
//! an immutable bounded ledger, receives a native challenge after validation,
//! then echoes it. The exporter requires the independently retained echo.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{MAX_BYTES, MAX_RECORDS, finite, read_named, save_named, uuid};

const MAX_CAPTURES: usize = 4096;
const MAX_PAGES: usize = MAX_CAPTURES / MAX_RECORDS;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Capture {
    observation_id: String,
    captured_ms: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Page {
    kind: String,
    schema_version: u32,
    runtime_id: String,
    census_id: String,
    page_index: usize,
    page_count: usize,
    records: Vec<Capture>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Close {
    kind: String,
    schema_version: u32,
    runtime_id: String,
    census_id: String,
    page_count: usize,
    boundary: String,
    clock: String,
    opened_ms: f64,
    closed_ms: f64,
    total_captured: u64,
    overflow: u64,
    failed_exports: u64,
    observation_ids_sha256: String,
    end_drain_deadline: EndDrainDeadline,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EndDrainDeadline {
    receipt_token: String,
    remaining_ms: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Confirm {
    kind: String,
    schema_version: u32,
    runtime_id: String,
    census_id: String,
    token: String,
    observation_ids_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PageRecord {
    kind: String,
    schema_version: u32,
    boot_id: String,
    clock: String,
    monotonic_ns: String,
    runtime_id: String,
    census_id: String,
    page: Page,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Closed {
    kind: String,
    schema_version: u32,
    boot_id: String,
    clock: String,
    monotonic_ns: String,
    runtime_id: String,
    census_id: String,
    token: String,
    native_deadline_ns: String,
    close: Close,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Confirmed {
    kind: String,
    schema_version: u32,
    boot_id: String,
    clock: String,
    monotonic_ns: String,
    runtime_id: String,
    census_id: String,
    token: String,
    observation_ids_sha256: String,
    receipt_sha256: Option<String>,
}

fn digest(ids: &BTreeSet<String>) -> String {
    Sha256::digest(
        ids.iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
            .as_bytes(),
    )
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect()
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_page(page: &Page) -> bool {
    let mut ids = BTreeSet::new();
    page.kind == "observer-census-page"
        && page.schema_version == 1
        && uuid(&page.runtime_id)
        && uuid(&page.census_id)
        && (1..=MAX_PAGES).contains(&page.page_count)
        && page.page_index < page.page_count
        && !page.records.is_empty()
        && page.records.len() <= MAX_RECORDS
        && (page.page_index + 1 == page.page_count || page.records.len() == MAX_RECORDS)
        && page.records.iter().all(|capture| {
            uuid(&capture.observation_id)
                && ids.insert(&capture.observation_id)
                && capture.captured_ms.is_none_or(finite)
        })
}

fn valid_close(close: &Close) -> bool {
    close.kind == "observer-census-close"
        && close.schema_version == 1
        && uuid(&close.runtime_id)
        && uuid(&close.census_id)
        && (1..=MAX_PAGES).contains(&close.page_count)
        && close.boundary == "SESSION_END"
        && close.clock == "performance.now"
        && finite(close.opened_ms)
        && finite(close.closed_ms)
        && close.opened_ms <= close.closed_ms
        && close.total_captured > 0
        && close.total_captured <= MAX_CAPTURES as u64
        && close.overflow == 0
        && valid_digest(&close.observation_ids_sha256)
        && uuid(&close.end_drain_deadline.receipt_token)
        && finite(close.end_drain_deadline.remaining_ms)
        && close.end_drain_deadline.remaining_ms > 0.0
        && close.end_drain_deadline.remaining_ms <= 100.0
}

fn anchored_deadline(close: &Close, native: &Value, boot: &str) -> Option<u64> {
    if native["kind"] != "helper-receipt"
        || native["schemaVersion"] != 1
        || native["clock"] != "CLOCK_UPTIME_RAW"
        || native["bootId"] != boot
        || native["runtimeId"] != close.runtime_id
        || native["token"] != close.end_drain_deadline.receipt_token
    {
        return None;
    }
    let stamp = native["monotonicNs"].as_str()?.parse::<u64>().ok()?;
    // The retained native point precedes the observer's subprocess-return
    // clock. Adding only the observer budget remaining at that return gives
    // a conservative native cutoff, without subtracting unrelated origins.
    // Platform rate/precision qualification remains mandatory separately;
    // this protocol never declares those clocks qualified.
    let remaining_ns = (close.end_drain_deadline.remaining_ms * 1_000_000.0).floor() as u64;
    stamp.checked_add(remaining_ns)
}

fn page_name(runtime: &str, census: &str, index: usize) -> String {
    format!("census-page-{runtime}-{census}-{index}.json")
}
fn close_name(runtime: &str, census: &str) -> String {
    format!("census-close-{runtime}-{census}.json")
}
fn confirmed_name(runtime: &str, census: &str) -> String {
    format!("census-confirmed-{runtime}-{census}.json")
}
fn echo_name(runtime: &str, census: &str) -> String {
    format!("census-echo-{runtime}-{census}.json")
}

fn bytes_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn receipt(runtime: &str, census: &str, status: &str, rest: Value) -> Value {
    let mut answer = json!({ "kind": "observer-census-receipt", "schemaVersion": 1,
        "accepted": true, "runtimeId": runtime, "censusId": census, "status": status });
    if let (Some(answer), Some(rest)) = (answer.as_object_mut(), rest.as_object()) {
        answer.extend(rest.clone());
    }
    answer
}

fn native_stamp(
    kind: &str,
    version: u32,
    clock: &str,
    actual_boot: &str,
    at: &str,
    boot: &str,
) -> Option<u64> {
    let parsed = at.parse::<u64>().ok()?;
    (kind.starts_with("observer-census-")
        && version == 1
        && clock == "CLOCK_UPTIME_RAW"
        && !boot.is_empty()
        && actual_boot == boot
        && parsed > 0)
        .then_some(parsed)
}

fn ledger(
    close: &Close,
    pages: &[PageRecord],
    boot: &str,
    closed_ns: u64,
) -> Option<(BTreeSet<String>, u64)> {
    if !valid_close(close) || pages.len() != close.page_count {
        return None;
    }
    let mut ordered = BTreeMap::new();
    for record in pages {
        let page = &record.page;
        let stamp = native_stamp(
            &record.kind,
            record.schema_version,
            &record.clock,
            &record.boot_id,
            &record.monotonic_ns,
            boot,
        )?;
        if record.kind != "observer-census-page-record"
            || !valid_page(page)
            || record.runtime_id != close.runtime_id
            || record.census_id != close.census_id
            || page.runtime_id != close.runtime_id
            || page.census_id != close.census_id
            || page.page_count != close.page_count
            || stamp > closed_ns
            || ordered.insert(page.page_index, page).is_some()
        {
            return None;
        }
    }
    let mut ids = BTreeSet::new();
    let mut missing_clocks = 0;
    let mut previous: Option<&str> = None;
    for index in 0..close.page_count {
        let page = ordered.get(&index)?;
        for record in &page.records {
            if previous.is_some_and(|old| old >= record.observation_id.as_str())
                || !ids.insert(record.observation_id.clone())
            {
                return None;
            }
            previous = Some(&record.observation_id);
            match record.captured_ms {
                Some(at) if close.opened_ms <= at && at <= close.closed_ms => {}
                None => missing_clocks += 1,
                Some(_) => return None,
            }
        }
    }
    (ids.len() as u64 == close.total_captured && digest(&ids) == close.observation_ids_sha256)
        .then_some((ids, missing_clocks))
}

fn page_answer(store: &Path, page: Page, boot: &str, stamp: u64) -> Option<Value> {
    if !valid_page(&page) {
        return None;
    }
    let name = page_name(&page.runtime_id, &page.census_id, page.page_index);
    if let Some(bytes) = read_named(store, &name).ok().flatten() {
        let existing: PageRecord = serde_json::from_slice(&bytes).ok()?;
        if existing.page != page || existing.boot_id != boot {
            return None;
        }
    } else {
        let record = PageRecord {
            kind: "observer-census-page-record".into(),
            schema_version: 1,
            boot_id: boot.into(),
            clock: "CLOCK_UPTIME_RAW".into(),
            monotonic_ns: stamp.to_string(),
            runtime_id: page.runtime_id.clone(),
            census_id: page.census_id.clone(),
            page: page.clone(),
        };
        save_named(store, &name, &serde_json::to_vec(&record).ok()?).ok()?;
    }
    Some(receipt(
        &page.runtime_id,
        &page.census_id,
        "PAGE_RECORDED",
        json!({ "pageIndex": page.page_index }),
    ))
}

fn close_answer(store: &Path, close: Close, boot: &str, stamp: u64) -> Option<Value> {
    if !valid_close(&close) {
        return None;
    }
    let native = read_named(
        store,
        &format!("native-{}.json", close.end_drain_deadline.receipt_token),
    )
    .ok()??;
    let cutoff = anchored_deadline(&close, &serde_json::from_slice(&native).ok()?, boot)?;
    if stamp > cutoff {
        return None;
    }
    let name = close_name(&close.runtime_id, &close.census_id);
    let closed = if let Some(bytes) = read_named(store, &name).ok().flatten() {
        let existing: Closed = serde_json::from_slice(&bytes).ok()?;
        if existing.close != close || existing.boot_id != boot {
            return None;
        }
        existing
    } else {
        let mut pages = Vec::new();
        for index in 0..close.page_count {
            let bytes = read_named(
                store,
                &page_name(&close.runtime_id, &close.census_id, index),
            )
            .ok()??;
            pages.push(serde_json::from_slice::<PageRecord>(&bytes).ok()?);
        }
        ledger(&close, &pages, boot, stamp)?;
        let closed = Closed {
            kind: "observer-census-closed".into(),
            schema_version: 1,
            boot_id: boot.into(),
            clock: "CLOCK_UPTIME_RAW".into(),
            monotonic_ns: stamp.to_string(),
            runtime_id: close.runtime_id.clone(),
            census_id: close.census_id.clone(),
            token: uuid::Uuid::new_v4().hyphenated().to_string(),
            native_deadline_ns: cutoff.to_string(),
            close: close.clone(),
        };
        save_named(store, &name, &serde_json::to_vec(&closed).ok()?).ok()?;
        closed
    };
    Some(receipt(
        &close.runtime_id,
        &close.census_id,
        "CLOSED",
        json!({ "token": closed.token,
        "totalCaptured": close.total_captured, "observationIdsSha256": close.observation_ids_sha256 }),
    ))
}

fn confirm_answer(
    store: &Path,
    confirm: Confirm,
    boot: &str,
    stamp: u64,
    now_ns: &mut impl FnMut() -> Option<u64>,
) -> Option<Value> {
    if confirm.kind != "observer-census-confirm"
        || confirm.schema_version != 1
        || !uuid(&confirm.runtime_id)
        || !uuid(&confirm.census_id)
        || !uuid(&confirm.token)
        || !valid_digest(&confirm.observation_ids_sha256)
    {
        return None;
    }
    let bytes = read_named(store, &close_name(&confirm.runtime_id, &confirm.census_id)).ok()??;
    let closed: Closed = serde_json::from_slice(&bytes).ok()?;
    let closed_ns = native_stamp(
        &closed.kind,
        closed.schema_version,
        &closed.clock,
        &closed.boot_id,
        &closed.monotonic_ns,
        boot,
    )?;
    if closed.kind != "observer-census-closed"
        || closed.runtime_id != confirm.runtime_id
        || closed.census_id != confirm.census_id
        || closed.token != confirm.token
        || closed.close.observation_ids_sha256 != confirm.observation_ids_sha256
        || closed_ns > stamp
        || closed
            .native_deadline_ns
            .parse::<u64>()
            .ok()
            .is_none_or(|cutoff| stamp > cutoff)
    {
        return None;
    }
    let name = confirmed_name(&confirm.runtime_id, &confirm.census_id);
    if let Some(bytes) = read_named(store, &name).ok().flatten() {
        let existing: Confirmed = serde_json::from_slice(&bytes).ok()?;
        if existing.runtime_id != confirm.runtime_id
            || existing.census_id != confirm.census_id
            || existing.token != confirm.token
            || existing.observation_ids_sha256 != confirm.observation_ids_sha256
            || existing.boot_id != boot
        {
            return None;
        }
    } else {
        // Persist the echoed challenge, then sample the actual completion
        // clock. A pre-fsync ACK stamp could conceal a late confirmation.
        let echo_path = echo_name(&confirm.runtime_id, &confirm.census_id);
        let echo_bytes = if let Some(bytes) = read_named(store, &echo_path).ok().flatten() {
            let echo: Confirmed = serde_json::from_slice(&bytes).ok()?;
            if echo.kind != "observer-census-confirm-received"
                || echo.runtime_id != confirm.runtime_id
                || echo.census_id != confirm.census_id
                || echo.token != confirm.token
                || echo.observation_ids_sha256 != confirm.observation_ids_sha256
                || echo.boot_id != boot
            {
                return None;
            }
            bytes
        } else {
            let echo = Confirmed {
                kind: "observer-census-confirm-received".into(),
                schema_version: 1,
                boot_id: boot.into(),
                clock: "CLOCK_UPTIME_RAW".into(),
                monotonic_ns: stamp.to_string(),
                runtime_id: confirm.runtime_id.clone(),
                census_id: confirm.census_id.clone(),
                token: confirm.token.clone(),
                observation_ids_sha256: confirm.observation_ids_sha256.clone(),
                receipt_sha256: None,
            };
            let bytes = serde_json::to_vec(&echo).ok()?;
            save_named(store, &echo_path, &bytes).ok()?;
            bytes
        };
        let completed_ns = now_ns()?;
        let cutoff = closed.native_deadline_ns.parse::<u64>().ok()?;
        if completed_ns < stamp || completed_ns > cutoff {
            return None;
        }
        let confirmed = Confirmed {
            kind: "observer-census-confirmed".into(),
            schema_version: 1,
            boot_id: boot.into(),
            clock: "CLOCK_UPTIME_RAW".into(),
            monotonic_ns: completed_ns.to_string(),
            runtime_id: confirm.runtime_id.clone(),
            census_id: confirm.census_id.clone(),
            token: confirm.token.clone(),
            observation_ids_sha256: confirm.observation_ids_sha256.clone(),
            receipt_sha256: Some(bytes_digest(&echo_bytes)),
        };
        save_named(store, &name, &serde_json::to_vec(&confirmed).ok()?).ok()?;
    }
    Some(receipt(
        &confirm.runtime_id,
        &confirm.census_id,
        "CONFIRMED",
        json!({ "token": confirm.token,
        "totalCaptured": closed.close.total_captured, "observationIdsSha256": confirm.observation_ids_sha256 }),
    ))
}

/// Persist one bounded census message in the development collector and
/// return its own typed metadata receipt. This never admits observations.
#[must_use]
pub fn answer_census(
    store: &Path,
    bytes: &[u8],
    boot: &str,
    native_ns: u64,
    mut now_ns: impl FnMut() -> Option<u64>,
) -> Option<Vec<u8>> {
    if bytes.len() > MAX_BYTES || boot.is_empty() || boot.len() > 128 || native_ns == 0 {
        return None;
    }
    let kind: Value = serde_json::from_slice(bytes).ok()?;
    let answer = match kind["kind"].as_str()? {
        "observer-census-page" => {
            page_answer(store, serde_json::from_slice(bytes).ok()?, boot, native_ns)
        }
        "observer-census-close" => {
            close_answer(store, serde_json::from_slice(bytes).ok()?, boot, native_ns)
        }
        "observer-census-confirm" => confirm_answer(
            store,
            serde_json::from_slice(bytes).ok()?,
            boot,
            native_ns,
            &mut now_ns,
        ),
        _ => None,
    }?;
    serde_json::to_vec(&answer).ok()
}

/// Reverify the actual native files when exporting a measurement. Pages,
/// close, and positive receipt confirmation must exist for every observed
/// source runtime. A retained prefix never becomes a closed population.
/// This deliberately supplies no conventional-hook invocation census.
pub fn observer_population_seal(
    records: &[Value],
    epochs: &BTreeSet<String>,
    boot: &str,
    from_ns: u64,
    through_ns: u64,
) -> Result<Value, String> {
    if epochs.is_empty() || epochs.iter().any(|epoch| !uuid(epoch)) || from_ns > through_ns {
        return Err("missing or invalid observer runtime population".into());
    }
    let mut all_ids = BTreeSet::new();
    let mut evidence = Vec::new();
    let mut failures = 0_u64;
    let mut total = 0_u64;
    for runtime in epochs {
        let selected: Vec<&Value> = records
            .iter()
            .filter(|value| value["runtimeId"].as_str() == Some(runtime))
            .collect();
        let close_values: Vec<_> = selected
            .iter()
            .filter(|value| value["kind"] == "observer-census-closed")
            .collect();
        if close_values.len() != 1 {
            return Err(format!(
                "observer {runtime}: missing or ambiguous final census close"
            ));
        }
        let closed: Closed = serde_json::from_value((*close_values[0]).clone())
            .map_err(|_| format!("observer {runtime}: invalid native close"))?;
        let stamp = native_stamp(
            &closed.kind,
            closed.schema_version,
            &closed.clock,
            &closed.boot_id,
            &closed.monotonic_ns,
            boot,
        )
        .filter(|stamp| from_ns <= *stamp && *stamp <= through_ns)
        .ok_or_else(|| format!("observer {runtime}: close outside same-boot measurement"))?;
        if !uuid(&closed.token)
            || closed.close.runtime_id != *runtime
            || closed.runtime_id != *runtime
            || closed.close.census_id != closed.census_id
        {
            return Err(format!("observer {runtime}: mismatched close identity"));
        }
        let anchors: Vec<_> = selected
            .iter()
            .filter(|value| {
                value["kind"] == "helper-receipt"
                    && value["token"] == closed.close.end_drain_deadline.receipt_token
            })
            .collect();
        if anchors.len() != 1 {
            return Err(format!(
                "observer {runtime}: missing or ambiguous native deadline anchor"
            ));
        }
        let cutoff = anchored_deadline(&closed.close, anchors[0], boot)
            .ok_or_else(|| format!("observer {runtime}: invalid native deadline anchor"))?;
        if closed.native_deadline_ns.parse::<u64>().ok() != Some(cutoff) || stamp > cutoff {
            return Err(format!(
                "observer {runtime}: close exceeded the original end-drain deadline"
            ));
        }
        let pages: Vec<PageRecord> = selected
            .iter()
            .filter(|value| value["kind"] == "observer-census-page-record")
            .map(|value| serde_json::from_value((*value).clone()))
            .collect::<Result<_, _>>()
            .map_err(|_| format!("observer {runtime}: invalid native page"))?;
        if pages.iter().any(|page| {
            page.monotonic_ns
                .parse::<u64>()
                .ok()
                .is_none_or(|stamp| stamp < from_ns)
        }) {
            return Err(format!(
                "observer {runtime}: census pages precede measurement start"
            ));
        }
        let (ids, missing_clocks) =
            ledger(&closed.close, &pages, boot, stamp).ok_or_else(|| {
                format!("observer {runtime}: missing page, UUID, count, clock or digest")
            })?;
        let confirms: Vec<_> = selected
            .iter()
            .filter(|value| value["kind"] == "observer-census-confirmed")
            .collect();
        if confirms.len() != 1 {
            return Err(format!(
                "observer {runtime}: final positive close receipt was not confirmed"
            ));
        }
        let confirm: Confirmed = serde_json::from_value((*confirms[0]).clone())
            .map_err(|_| format!("observer {runtime}: invalid final confirmation"))?;
        let confirmed_at = native_stamp(
            &confirm.kind,
            confirm.schema_version,
            &confirm.clock,
            &confirm.boot_id,
            &confirm.monotonic_ns,
            boot,
        )
        .filter(|at| stamp <= *at && *at <= through_ns && *at <= cutoff);
        if confirmed_at.is_none()
            || confirm.runtime_id != *runtime
            || confirm.census_id != closed.census_id
            || confirm.token != closed.token
            || confirm.observation_ids_sha256 != closed.close.observation_ids_sha256
        {
            return Err(format!(
                "observer {runtime}: wrong or out-of-window confirmation"
            ));
        }
        let echoes: Vec<_> = selected
            .iter()
            .filter(|value| value["kind"] == "observer-census-confirm-received")
            .collect();
        if echoes.len() != 1 {
            return Err(format!(
                "observer {runtime}: missing or ambiguous durable confirmation receipt"
            ));
        }
        let echo: Confirmed = serde_json::from_value((*echoes[0]).clone())
            .map_err(|_| format!("observer {runtime}: invalid durable confirmation receipt"))?;
        let echo_at = native_stamp(
            &echo.kind,
            echo.schema_version,
            &echo.clock,
            &echo.boot_id,
            &echo.monotonic_ns,
            boot,
        );
        let echo_bytes =
            serde_json::to_vec(&echo).map_err(|_| "cannot serialize confirmation receipt")?;
        if echo_at
            .is_none_or(|at| at < stamp || at > confirmed_at.expect("checked confirmation time"))
            || echo.runtime_id != *runtime
            || echo.census_id != closed.census_id
            || echo.token != closed.token
            || echo.observation_ids_sha256 != closed.close.observation_ids_sha256
            || echo.receipt_sha256.is_some()
            || confirm.receipt_sha256.as_deref() != Some(bytes_digest(&echo_bytes).as_str())
        {
            return Err(format!(
                "observer {runtime}: durable confirmation receipt does not match its completion witness"
            ));
        }
        // Real helper receipts name the UUIDs submitted by that runtime,
        // independently of optional runtime sample export. A dropped final
        // sample, late callback or another batch cannot shrink this ledger.
        for record in &selected {
            if record["kind"] != "helper-receipt" {
                continue;
            }
            if record["bootId"] != boot
                || record["monotonicNs"]
                    .as_str()
                    .and_then(|value| value.parse::<u64>().ok())
                    .is_none_or(|at| at < from_ns || at > through_ns)
            {
                return Err(format!(
                    "observer {runtime}: helper batch outside declared source window"
                ));
            }
            let submitted = record["requestObservationIds"].as_array().ok_or_else(|| {
                format!("observer {runtime}: missing independent submitted UUID ledger")
            })?;
            if submitted
                .iter()
                .any(|id| id.as_str().is_none_or(|id| !ids.contains(id)))
            {
                return Err(format!(
                    "observer {runtime}: native helper saw a UUID absent from final source census"
                ));
            }
        }
        for id in ids {
            if !all_ids.insert(id) {
                return Err("one observer capture UUID occurs in multiple source runtimes".into());
            }
        }
        failures = failures
            .checked_add(closed.close.failed_exports)
            .and_then(|value| value.checked_add(missing_clocks))
            .ok_or("telemetry failure counter overflow")?;
        total = total
            .checked_add(closed.close.total_captured)
            .ok_or("capture counter overflow")?;
        evidence.push(format!(
            "native-helper:observer-census-confirmed:{}:{}",
            closed.census_id, closed.token
        ));
    }
    Ok(
        json!({ "source": "claude.observer", "boundary": "CLOSED_CAPTURE_WINDOW", "sealed": true,
        "sourceBoundary": "SESSION_END", "bootId": boot, "evidence": evidence,
        "totalCaptured": total, "observationIdsSha256": digest(&all_ids), "telemetryFailures": failures,
        "runtimeIds": epochs.iter().collect::<Vec<_>>() }),
    )
}

#[cfg(test)]
#[path = "latency_census_tests.rs"]
mod tests;
