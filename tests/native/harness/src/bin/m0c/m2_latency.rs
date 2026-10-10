//! F4 metadata collector. It does not launch/stop apps or providers, install
//! integrations, change settings, or focus a native window. A separate safe
//! native fixture owns the Dev store/session and enables the qualification
//! helper flag. All measurement data and incomplete conditions are retained.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use threadspace_harness::evidence::{Run, sha256_file};

use crate::{ctx::Ctx, m2};

fn root(ctx: &Ctx) -> Result<PathBuf, String> {
    if ctx.id.app_identifier != "ai.scalinity.threadspace.dev" {
        return Err("F4 collector requires the development identity".into());
    }
    Ok(ctx.repo.join("evidence/M2/remediation-4/f4/native"))
}

fn monotonic_ns() -> Result<u64, String> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: ts is valid writable storage. macOS CLOCK_UPTIME_RAW is the
    // same clock explicitly used by the capture helper and COMMIT bracket.
    let ok = unsafe { libc::clock_gettime(libc::CLOCK_UPTIME_RAW, &mut ts) } == 0;
    if !ok || ts.tv_sec < 0 || !(0..1_000_000_000).contains(&ts.tv_nsec) {
        return Err("native monotonic clock unavailable".into());
    }
    (ts.tv_sec as u64)
        .checked_mul(1_000_000_000)
        .and_then(|value| value.checked_add(ts.tv_nsec as u64))
        .ok_or_else(|| "native monotonic clock overflow".into())
}

fn calibration(ctx: &Ctx, boot: &str) -> Result<Vec<Value>, String> {
    let mut result = Vec::new();
    for _ in 0..5 {
        let before = monotonic_ns()?;
        let reply = m2::ui(ctx, "m2-latency-clock", json!({}))?;
        let after = monotonic_ns()?;
        result.push(json!({ "bootId": boot, "clock": "CLOCK_UPTIME_RAW",
            "nativeBeforeNs": before.to_string(), "nativeAfterNs": after.to_string(), "reply": reply["result"] }));
    }
    Ok(result)
}

pub fn begin(ctx: &Ctx) -> Result<Value, String> {
    let dir = root(ctx)?;
    let run = Run::create(&dir, "capture", "dev").map_err(|error| error.to_string())?;
    let boot = threadspace_surfaces_macos::process::boot_session_id()
        .map_err(|error| format!("{error:?}"))?;
    let started = monotonic_ns()?;
    let cursor = m2::journal_cursor(ctx);
    let view = m2::ui(ctx, "m2-latency-start", json!({}))?;
    let clocks = calibration(ctx, &boot)?;
    let provenance = json!({
        "environment": ctx.environment(),
        "helperSha256": ctx.id.companion_executable.parent().and_then(|dir| sha256_file(&dir.join("threadspace-hook"))),
        "head": threadspace_harness::run::run("/usr/bin/git", &["rev-parse", "HEAD"], std::time::Duration::from_secs(5)).stdout.trim(),
        "gitStatus": threadspace_harness::run::run("/usr/bin/git", &["status", "--porcelain"], std::time::Duration::from_secs(5)).stdout,
    });
    let value = json!({
        "kind": "M2_LATENCY_START", "schemaVersion": 2,
        "appIdentifier": ctx.id.app_identifier, "bootId": boot,
        "startNativeNs": started.to_string(), "afterCursor": cursor.to_string(),
        "uiStart": view["result"], "uiCalibrations": clocks, "provenance": provenance,
        "sourceWindow": "Start the owned observer runtime after this boundary; end its original engine/core Session before exporting. Missing runtime closure is INCOMPLETE.",
    });
    run.write_json("start.json", &value)
        .map_err(|error| error.to_string())?;
    Ok(
        json!({ "status": "MEASUREMENT_STARTED", "runDirectory": run.dir, "qualificationVerdict": "INCOMPLETE" }),
    )
}

fn read_json(path: &Path, limit: u64) -> Result<Value, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > limit {
        return Err(format!("invalid bounded metadata file: {}", path.display()));
    }
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

pub fn end(ctx: &Ctx, directory: &Path) -> Result<Value, String> {
    let root = root(ctx)?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let directory = directory
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !directory.starts_with(root) || directory.join("raw.json").exists() {
        return Err("F4 end requires a new retained start directory under this checkout's remediation evidence".into());
    }
    let start = read_json(&directory.join("start.json"), 1024 * 1024)?;
    if start["kind"] != "M2_LATENCY_START" || start["appIdentifier"] != ctx.id.app_identifier {
        return Err("F4 start identity mismatch".into());
    }
    let boot = threadspace_surfaces_macos::process::boot_session_id()
        .map_err(|error| format!("{error:?}"))?;
    if start["bootId"].as_str() != Some(&boot) {
        return Err("boot changed during measurement".into());
    }
    let from = start["startNativeNs"]
        .as_str()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or("missing native start")?;
    let after = start["afterCursor"]
        .as_str()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or("missing start cursor")?;
    let stop = m2::ui(ctx, "m2-latency-stop", json!({}))?;
    let ended = monotonic_ns()?;
    let before_cursor = m2::journal_cursor(ctx);
    let mut pages = vec![stop["result"].clone()];
    while let Some(offset) = pages.last().and_then(|page| page["nextOffset"].as_u64()) {
        if pages.len() >= 41 {
            return Err("DOM collector exceeds its declared 4096-record bound".into());
        }
        let page = m2::ui(ctx, "m2-latency", json!({ "offset": offset, "limit": 100 }))?;
        pages.push(page["result"].clone());
    }
    let mut clocks = start["uiCalibrations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    clocks.extend(calibration(ctx, &boot)?);

    let mut collector_errors = Vec::new();
    let mut all_helper = Vec::new();
    let helper_dir = ctx.id.agent.store_dir.join("qualification/m2-latency");
    match std::fs::read_dir(&helper_dir) {
        Ok(entries) => {
            for (index, entry) in entries.enumerate() {
                if index >= 1024 {
                    collector_errors.push("helper collector full or exceeded its bound".to_owned());
                    break;
                }
                let path = entry.map_err(|error| error.to_string())?.path();
                match read_json(&path, 64 * 1024) {
                    Ok(value) => all_helper.push(value),
                    Err(error) => collector_errors.push(error),
                }
            }
        }
        Err(error) => collector_errors.push(format!("missing helper telemetry: {error}")),
    }
    let in_window = |value: &Value| {
        value
            .as_str()
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|at| from <= at && at <= ended)
    };
    let tokens: std::collections::BTreeSet<String> = all_helper
        .iter()
        .filter(|value| {
            value["kind"] == "helper-receipt"
                && value["bootId"] == boot
                && in_window(&value["monotonicNs"])
        })
        .filter_map(|value| value["token"].as_str().map(str::to_owned))
        .collect();
    let epochs: std::collections::BTreeSet<String> = all_helper
        .iter()
        .filter(|value| match value["kind"].as_str() {
            Some("observer-clock-bracket") => value["nativeToken"]
                .as_str()
                .is_some_and(|token| tokens.contains(token)),
            Some(
                "helper-receipt"
                | "observer-census-page-record"
                | "observer-census-closed"
                | "observer-census-confirmed"
                | "observer-census-confirm-received",
            ) => value["bootId"] == boot && in_window(&value["monotonicNs"]),
            _ => false,
        })
        .filter_map(|value| value["runtimeId"].as_str().map(str::to_owned))
        .collect();
    // The population is reconstructed from the actual immutable source
    // ledger and independently retained native close/confirmation records.
    // Counting successful sample files alone cannot establish a final tail.
    let mut population_seals = Vec::new();
    match threadspace_relay::latency::observer_population_seal(
        &all_helper,
        &epochs,
        &boot,
        from,
        ended,
    ) {
        Ok(seal) => population_seals.push(seal),
        Err(error) => collector_errors.push(format!("observer source census incomplete: {error}")),
    }
    // Conventional hooks are separate short-lived invocations. Neither the
    // stored hook rows nor the journal can count an invocation when both
    // delivery/spool and telemetry failed. No independent host invocation
    // witness exists in this adapter, so never manufacture a hook seal from
    // those surviving files. This requires a qualified host-side invocation
    // census (including zero-output failures) before normal latency can PASS.
    let hook_census = json!({ "status": "INCOMPLETE", "source": "claude.hook", "sealed": false,
        "reason": "No independent host invocation census: a conventional hook that fails both delivery/spool and telemetry leaves no counted candidate. Retained hook files are only a capture ledger, not a closed source population." });
    let helpers: Vec<Value> = all_helper
        .into_iter()
        .filter(|value| match value["kind"].as_str() {
            Some("hook-capture") => {
                value["capture"]["bootId"] == boot && in_window(&value["capture"]["monotonicNs"])
            }
            Some("helper-receipt") => value["token"]
                .as_str()
                .is_some_and(|token| tokens.contains(token)),
            Some("observer-clock-bracket") => value["runtimeId"]
                .as_str()
                .is_some_and(|epoch| epochs.contains(epoch)),
            Some(
                "observer-census-page-record"
                | "observer-census-closed"
                | "observer-census-confirmed"
                | "observer-census-confirm-received",
            ) => value["runtimeId"]
                .as_str()
                .is_some_and(|epoch| epochs.contains(epoch)),
            _ => false,
        })
        .collect();

    let file = std::fs::File::open(ctx.id.companion_log()).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() > 64 * 1024 * 1024 {
        return Err("companion measurement log exceeds 64 MiB; retain and extract its bounded run explicitly".into());
    }
    let mut commits = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|error| error.to_string())?;
        match serde_json::from_str::<Value>(&line) {
            Ok(value) if value["event"] == "M2_COMMITTED_MEASUREMENT" => {
                // No payload text is exported. Source/observation IDs and
                // exact native COMMIT brackets define the run population.
                let cursor = value["cursor"]
                    .as_str()
                    .and_then(|value| value.parse::<u64>().ok());
                if cursor.is_some_and(|cursor| after < cursor && cursor <= before_cursor)
                    || in_window(&value["commit"]["endNs"])
                {
                    commits.push(value);
                }
            }
            Ok(_) => {}
            Err(_) => collector_errors
                .push("unparseable companion log line; raw log must be investigated".into()),
        }
    }
    let raw = json!({
        "schemaVersion": 2, "executionKind": "NATIVE", "bootId": boot,
        "clockQualification": { "nativeClock": "CLOCK_UPTIME_RAW", "qualified": false,
            "maximumRateErrorPpm": null, "precisionNs": null, "evidence": [],
            "reason": "Clock brackets are raw evidence. Platform/runtime clock-rate and precision qualification must be independently established before setting a bound." },
        "start": start, "endNativeNs": ended.to_string(), "provenanceEnd": ctx.environment(),
        "helperRecords": helpers, "commitRecords": commits, "domPages": pages,
        "uiCalibrations": clocks, "collectorErrors": collector_errors,
        "populationSeals": population_seals, "hookPopulationCensus": hook_census,
    });
    let run = Run { dir: directory };
    run.write_json("raw.json", &raw)
        .map_err(|error| error.to_string())?;
    Ok(
        json!({ "status": "RAW_MEASUREMENT_RETAINED", "runDirectory": run.dir, "normalPathPass": false,
        "reason": "Observer census files are verified when present. Conventional-hook invocation census and positive platform clock qualification remain required; raw collection is not a PASS." }),
    )
}
