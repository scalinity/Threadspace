//! Qualification-only native post-DOM stamps; no flow-control or journal ACK.
use serde_json::{Value, json};
use threadspace_contracts::composed::{Context, Request, valid_context, valid_witness};
use threadspace_contracts::control::{ControlError, ControlErrorCode};
use threadspace_contracts::cursor::parse_cursor;

pub struct Run {
    context: Context,
    cursor: String,
    stream_seq: u32,
    boot: String,
    sleeps: u32,
    wakes: u32,
    count: u32,
    failures: u32,
    closed: bool,
    last_ns: u64,
    ids: std::collections::BTreeSet<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use threadspace_contracts::composed::Witness;

    fn context() -> Context {
        let id = || uuid::Uuid::new_v4().to_string();
        Context { run_id: id(), dom_runtime_id: id(), native_incarnation: id(), subscription_id: id(),
            view_epoch: id(), core_generation: id(), store_generation: id() }
    }
    fn started() -> (Option<Run>, Context, Witness) {
        let context = context();
        let mut slot = None;
        apply(&mut slot, &Request::Start { context: context.clone(), cursor: "7".into(), stream_seq: 3 }, "boot", 0, 0, native_ns).expect("start");
        let witness = Witness { schema_version: 3, witness_id: uuid::Uuid::new_v4().to_string(), context: context.clone(), from_cursor: "7".into(), cursor: "8".into(),
            from_stream_seq: 3, stream_seq: 4, revisions: vec![], pending_hydration: 0, visible: true };
        (slot, context, witness)
    }
    #[test]
    fn composed_actual_native_stamp_is_after_validation_and_retains_delay() {
        let (mut slot, context, witness) = started();
        let before = native_ns().expect("actual native counter");
        std::thread::sleep(std::time::Duration::from_millis(5));
        let ack = apply(&mut slot, &Request::Witness { witness }, "boot", 0, 0, native_ns).expect("ack");
        let at = ack["monotonicNs"].as_str().expect("stamp").parse::<u64>().expect("ns");
        let after = native_ns().expect("actual native counter");
        assert!(before <= at && at <= after);
        assert!(at - before >= 4_000_000, "actual late stamp, not a clamped deadline");
        let close = apply(&mut slot, &Request::Close { context, total_witnesses: 1, failures: 0 }, "boot", 0, 0, native_ns).expect("census");
        assert_eq!(close["closed"], true);
    }
    #[test]
    fn composed_refuses_stale_context_hydration_power_reset_and_lost_census() {
        for mode in ["context", "hydration", "power", "reset", "census"] {
            let (mut slot, context, mut witness) = started();
            if mode == "context" { witness.context.native_incarnation = uuid::Uuid::new_v4().to_string(); }
            if mode == "hydration" { witness.pending_hydration = 1; }
            let request = if mode == "census" { Request::Close { context: context.clone(), total_witnesses: 1, failures: 0 } } else { Request::Witness { witness } };
            let mut read = false;
            let result = apply(&mut slot, &request, "boot", u32::from(mode == "power"), 0, || { read = true; if mode == "reset" { Some(0) } else { native_ns() } });
            assert!(result.is_err(), "{mode}");
            if mode != "reset" { assert!(!read, "stamp must follow validation: {mode}"); }
            assert!(apply(&mut slot, &Request::Close { context, total_witnesses: 0, failures: 0 }, "boot", 0, 0, native_ns).is_err(), "failed evidence cannot close as PASS");
        }
    }
    #[test]
    fn composed_refuses_overflow_duplicate_and_evidence_after_close() {
        let (mut slot, context, witness) = started();
        apply(&mut slot, &Request::Witness { witness: witness.clone() }, "boot", 0, 0, native_ns).expect("ack");
        assert!(apply(&mut slot, &Request::Witness { witness }, "boot", 0, 0, native_ns).is_err());
        slot.as_mut().expect("run").count = 4096;
        let (_, _, mut extra) = started(); extra.context = context.clone();
        assert!(apply(&mut slot, &Request::Witness { witness: extra }, "boot", 0, 0, native_ns).is_err());
        let (mut slot, context, witness) = started();
        apply(&mut slot, &Request::Close { context, total_witnesses: 0, failures: 0 }, "boot", 0, 0, native_ns).expect("empty close");
        assert!(apply(&mut slot, &Request::Witness { witness }, "boot", 0, 0, native_ns).is_err());
    }
}

pub fn native_ns() -> Option<u64> {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: ts is writable; this is the journal's same-boot native clock.
    if unsafe { libc::clock_gettime(libc::CLOCK_UPTIME_RAW, &mut ts) } != 0 || ts.tv_sec < 0 || !(0..1_000_000_000).contains(&ts.tv_nsec) { return None; }
    (ts.tv_sec as u64).checked_mul(1_000_000_000)?.checked_add(ts.tv_nsec as u64)
}

fn refused(detail: &str) -> ControlError { ControlError::new(ControlErrorCode::BadRequest, detail) }

pub fn apply(slot: &mut Option<Run>, request: &Request, boot: &str, sleeps: u32, wakes: u32,
    now: impl FnOnce() -> Option<u64>) -> Result<Value, ControlError> {
    if !valid_context(request.context()) { return Err(refused("invalid composed context")); }
    if let Request::Start { context, cursor, stream_seq } = request {
        if slot.is_some() || parse_cursor(cursor).is_none() || *stream_seq == 0 { return Err(refused("ambiguous composed start")); }
        let at = now().ok_or_else(|| refused("native clock unavailable"))?;
        *slot = Some(Run { context: context.clone(), cursor: cursor.clone(), stream_seq: *stream_seq,
            boot: boot.into(), sleeps, wakes, count: 0, failures: 0, closed: false, last_ns: at, ids: Default::default() });
        return Ok(json!({"kind":"composed-duration-start", "schemaVersion":3, "context":context, "cursor":cursor, "streamSeq":stream_seq,
            "bootId":boot, "clock":"CLOCK_UPTIME_RAW", "monotonicNs":at.to_string(), "sleeps":sleeps, "wakes":wakes}));
    }
    let run = slot.as_mut().ok_or_else(|| refused("missing composed start"))?;
    let validate = || -> Result<(), ControlError> {
        if run.closed || request.context() != &run.context || run.boot != boot || run.sleeps != sleeps || run.wakes != wakes {
            return Err(refused("stale composed context or power discontinuity"));
        }
        match request {
            Request::Witness { witness } => {
                if !valid_witness(witness) || witness.from_cursor != run.cursor || witness.from_stream_seq != run.stream_seq
                    || run.count >= 4096 || run.ids.contains(&witness.witness_id) { return Err(refused("invalid, duplicate or overflowing DOM witness")); }
            }
            Request::Close { total_witnesses, failures, .. } => {
                if *total_witnesses != run.count || *failures != 0 || run.failures != 0 { return Err(refused("incomplete composed DOM census")); }
            }
            Request::Start { .. } => unreachable!(),
        }
        Ok(())
    };
    if let Err(error) = validate() { run.failures += 1; return Err(error); }
    // Read only after immutable context, coverage shape, census and power
    // validation. Transport/queue delay remains in this actual late stamp.
    let at = match now().filter(|at| *at >= run.last_ns) {
        Some(at) => at,
        None => { run.failures += 1; return Err(refused("native reset or missing clock")); }
    };
    run.last_ns = at;
    match request {
        Request::Witness { witness } => {
            run.ids.insert(witness.witness_id.clone());
            run.count += 1;
            Ok(json!({"kind":"composed-dom-receipt", "schemaVersion":3, "context":run.context, "witness":witness,
                "clock":"CLOCK_UPTIME_RAW", "bootId":boot, "monotonicNs":at.to_string(), "ordinal":run.count}))
        }
        Request::Close { .. } => {
            run.closed = true;
            Ok(json!({"kind":"composed-dom-census", "schemaVersion":3, "context":run.context,
                "clock":"CLOCK_UPTIME_RAW", "bootId":boot, "monotonicNs":at.to_string(),
                "totalWitnesses":run.count, "failures":run.failures, "closed":true}))
        }
        _ => unreachable!(),
    }
}
