//! Qualification-only brackets around the actual SQLite COMMIT call.
//! These are measurements, never canonical observations or replay inputs.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitTiming {
    pub boundary: String,
    pub clock: String,
    pub begin_ns: String,
    pub end_ns: String,
}

pub(crate) fn clock_name() -> &'static str {
    if cfg!(target_os = "macos") { "CLOCK_UPTIME_RAW" } else { "CLOCK_MONOTONIC" }
}

pub(crate) fn now_ns() -> Option<u64> {
    #[cfg(target_os = "macos")]
    let clock = libc::CLOCK_UPTIME_RAW;
    #[cfg(not(target_os = "macos"))]
    let clock = libc::CLOCK_MONOTONIC;
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: ts is a valid writable timespec; no wall clock is involved.
    let ok = unsafe { libc::clock_gettime(clock, &mut ts) } == 0;
    if !ok || ts.tv_sec < 0 || !(0..1_000_000_000).contains(&ts.tv_nsec) { return None; }
    (ts.tv_sec as u64).checked_mul(1_000_000_000)?.checked_add(ts.tv_nsec as u64)
}

pub(crate) fn finish(begin: Option<u64>) -> Option<CommitTiming> {
    let end = now_ns()?;
    let begin = begin.filter(|begin| *begin <= end)?;
    Some(CommitTiming {
        boundary: "SQLITE_COMMIT_CALL".into(),
        clock: clock_name().into(),
        begin_ns: begin.to_string(),
        end_ns: end.to_string(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn commit_bracket_is_monotonic_and_explicit() {
        let value = super::finish(super::now_ns()).expect("qualified native clock");
        assert_eq!(value.boundary, "SQLITE_COMMIT_CALL");
        assert!(value.begin_ns.parse::<u64>().expect("begin") <= value.end_ns.parse::<u64>().expect("end"));
        assert!(super::finish(None).is_none(), "missing timing is not a zero-duration success");
        assert!(super::finish(Some(u64::MAX)).is_none(), "incompatible/reversed time is rejected");
    }

    /// SQLite calls this inside COMMIT, before the transaction is durable.
    /// The tiny test-only delay makes a pre-COMMIT end stamp distinguishable
    /// even on a coarse clock. No database operation or panic occurs here.
    unsafe extern "C" fn commit_witness(context: *mut std::ffi::c_void) -> std::ffi::c_int {
        std::thread::sleep(std::time::Duration::from_millis(5));
        // SAFETY: the test owns this live Box until it unregisters the hook;
        // SQLite invokes the hook synchronously on the same connection.
        let stamp = unsafe { &*context.cast::<std::sync::atomic::AtomicU64>() };
        stamp.store(super::now_ns().unwrap_or(0), std::sync::atomic::Ordering::SeqCst);
        0
    }

    #[test]
    fn commit_end_follows_actual_sqlite_commit_hook_not_writer_receipt() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use threadspace_contracts::canonical::envelope::{CaptureClock, ClockQuality, ObservationEnvelope};
        use threadspace_contracts::canonical::fact::Delivery;
        use threadspace_state_engine::normalize::Normalized;
        use crate::{EnvelopeAdmission, Journal};

        let directory = std::env::temp_dir().join(format!("threadspace-f4-boundary-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).expect("acquire disposable fixture");
        let mut journal = Journal::open(&directory.join("journal.sqlite3"), "f4-boundary", 1).expect("journal");
        let mut stamp = Box::new(AtomicU64::new(0));
        // SAFETY: this is the connection's only hook. Context remains alive
        // and stationary, and is removed before either connection or Box is
        // dropped. The callback does not reenter SQLite or unwind.
        unsafe {
            rusqlite::ffi::sqlite3_commit_hook(journal.conn.handle(), Some(commit_witness),
                (&mut *stamp as *mut AtomicU64).cast());
        }
        let envelope = ObservationEnvelope {
            schema_version: 1, observation_id: uuid::Uuid::new_v4().to_string(),
            source_id: "f4.boundary".into(), source_epoch: "test".into(),
            source_sequence: None, sequence_meaning: None, callback_entry_sequence: None,
            callback_result_sequence: None, adapter_id: "f4.boundary".into(), adapter_version: "1".into(),
            provider_version: None, native_event: "BoundaryFixture".into(), session_key: None,
            actor_native_id: None, native_turn_id: None, native_prompt_id: None,
            native_occurrence_id: None, activation_ref: None,
            captured_at: CaptureClock { endpoint_id: None, boot_id: Some("portable".into()),
                monotonic_ns: None, wall_time_ms: 1, clock_quality: ClockQuality::ReceiptOnly },
            evidence: vec![], payload: serde_json::json!({}),
        };
        let result = journal.admit_batch(&[EnvelopeAdmission {
            envelope: &envelope,
            normalized: Normalized { drafts: vec![], retained: serde_json::json!({}), unsupported: None },
        }], Delivery::Live, 2);
        // SAFETY: unregister the callback before inspecting the result, so
        // an assertion failure cannot leave a dangling callback pointer.
        unsafe { rusqlite::ffi::sqlite3_commit_hook(journal.conn.handle(), None, std::ptr::null_mut()); }
        let timing = result.expect("admission").commit_timing.expect("COMMIT timing");
        let inside = stamp.load(Ordering::SeqCst);
        assert!(inside > 0, "the real SQLite COMMIT hook must execute");
        let begin = timing.begin_ns.parse::<u64>().expect("begin");
        let end = timing.end_ns.parse::<u64>().expect("end");
        assert!(begin <= inside && inside <= end,
            "COMMIT end must follow SQLite's actual commit hook: begin={begin}, inside={inside}, end={end}");
        drop(journal);
        std::fs::remove_dir_all(directory).expect("remove only acquired fixture");
    }
}
