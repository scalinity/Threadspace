//! Generates the M0-era (schema 2) journal fixture with the accepted M0C
//! journal code itself (commit cd9e37645adf7e6b5f74ab7f0baa5197d8e08b54).
//! Run from a checkout of that commit as an integration test of
//! `threadspace-journal` with `--features qualification` and
//! `THREADSPACE_M0_FIXTURE_OUT=<path>`; it writes a store exercising every
//! M0 write path: the M0A fixture, M0B discovery (bound, unproven and ended
//! activations, a PID reused with a new birth), a recorded route,
//! qualification attention with an outbox intent, an acknowledgement, a
//! resolution and a notification outcome.

use threadspace_contracts::projection::NotificationState;
use threadspace_contracts::route::{
    InputReadiness, RouteEvidence, RouteResult, SessionVerification, SurfaceResult,
};
use threadspace_journal::{
    ActivationChange, DiscoveryApplication, Journal, ObservedSessionRecord, ProcessRecord,
    SurfaceRecord,
};

const NOW: i64 = 1_791_000_000_000;

fn process(pid: u32, birth: u64) -> ProcessRecord {
    ProcessRecord {
        boot_id: "boot-m0".into(),
        pid,
        start_seconds: birth,
        start_microseconds: 7,
        executable: "/opt/fixture/claude/2.1.291#1:2".into(),
    }
}

fn surface(tty: &str, device: u32) -> SurfaceRecord {
    SurfaceRecord {
        tty: tty.into(),
        device,
        terminal_generation: "95390@1791000000.000001".into(),
        window_hint: 7,
        tab_hint: 1,
        proof: serde_json::json!({ "matches": 1 }),
    }
}

fn observed(id: &str) -> ObservedSessionRecord {
    ObservedSessionRecord {
        native_session_id: id.into(),
        kind: Some("interactive".into()),
        display_name: Some(format!("worker {id}")),
        status: Some("idle".into()),
        waiting_for: None,
    }
}

fn application(sessions: Vec<ObservedSessionRecord>, changes: Vec<ActivationChange>) -> DiscoveryApplication {
    DiscoveryApplication {
        provider: "claude".into(),
        profile_ref: "claude-cli:~/.claude".into(),
        sessions,
        changes,
        payload: serde_json::json!({ "fixture": "m0-store-v2" }),
    }
}

#[test]
fn write_m0_store_fixture() {
    let Some(out) = std::env::var_os("THREADSPACE_M0_FIXTURE_OUT") else { return };
    let path = std::path::PathBuf::from(out);
    let _ = std::fs::remove_file(&path);
    let mut j = Journal::open(&path, "m0-epoch", NOW).expect("open");
    j.apply_discovery(
        &application(
            vec![observed("A"), observed("B")],
            vec![
                ActivationChange::Start { native_session_id: "A".into(), process: process(11, 1000), device: 5, surface: Ok(surface("/dev/ttys011", 5)) },
                ActivationChange::Start { native_session_id: "B".into(), process: process(12, 1001), device: 6, surface: Err("NO_MATCHING_TAB".into()) },
            ],
        ),
        NOW + 1,
    )
    .expect("discovery");
    let a1 = j.live_executions("claude").expect("live").into_iter().find(|e| e.native_session_id == "A").expect("A").execution_id;
    j.apply_discovery(
        &application(
            vec![observed("A"), observed("B")],
            vec![
                ActivationChange::End { execution_id: a1, reason: "PROCESS_EXITED".into() },
                ActivationChange::Start { native_session_id: "A".into(), process: process(11, 2000), device: 5, surface: Ok(surface("/dev/ttys011", 5)) },
            ],
        ),
        NOW + 2,
    )
    .expect("pid reuse");
    j.apply_discovery(&application(vec![observed("A")], vec![]), NOW + 3).expect("B leaves inventory");
    let (_, snapshot) = j.snapshot().expect("snapshot");
    let a = snapshot.sessions.iter().find(|s| s.native_session_id == "A").expect("A").session_id.clone();
    j.record_route(
        &RouteResult {
            request_id: "6f1b3c2e-4a5d-4e6f-8a9b-0c1d2e3f4a5b".into(),
            session_id: a,
            surface_result: SurfaceResult::ExactNativeSurface,
            session_verification: SessionVerification::CurrentNativeRevalidated,
            input_readiness: InputReadiness::ForegroundCompatible,
            binding_id: None,
            validated_binding_revision: None,
            reason_code: "OK".into(),
            focus_performed: true,
            started_at_ms: NOW + 4,
            latency_ms: 300,
            choices: vec![],
            evidence: RouteEvidence::default(),
        },
        NOW + 5,
    )
    .expect("route");
    let raised = j.raise_qualification_attention("m0 store fixture", NOW + 6).expect("raise");
    j.acknowledge_attention("cmd-m0-ack", &raised.intent.attention_id, None, NOW + 7).expect("ack");
    j.record_notification_state(&raised.intent.request_id, &NotificationState::Submitted, "submitted", NOW + 8).expect("notification");
    let fixture_item = snapshot.attention.first().expect("fixture attention").attention_id.clone();
    j.resolve_attention("cmd-m0-resolve", &fixture_item, None, "handled before M1", NOW + 9).expect("resolve");
}
