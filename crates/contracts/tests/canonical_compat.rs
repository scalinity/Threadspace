//! Records written by an earlier M1 build of the same reducer version load:
//! fields added since default (a wait scope's owner decisions, a binding's
//! proof point).

use threadspace_contracts::canonical::records::{SurfaceBindingRecord, WaitScopeRecord};

#[test]
fn an_earlier_wait_scope_without_owner_decisions_loads() {
    let json = serde_json::json!({
        "key": "k", "sessionId": "s", "actorId": null, "executionId": null, "turnId": null,
        "category": "INPUT", "subtypes": [], "generation": null, "positives": [], "clears": [],
        "unorderedPositives": 0, "unorderedClears": 0, "episodes": [], "createdCursor": 1, "revision": 1,
    });
    let scope: WaitScopeRecord = serde_json::from_value(json).expect("loads");
    assert!(scope.owner_decisions.is_empty());
}

#[test]
fn an_earlier_binding_without_a_proof_point_loads() {
    let json = serde_json::json!({
        "id": "b", "sessionId": "s", "executionId": "e", "surfaceId": "f", "processId": null,
        "method": null, "executableIdentity": null, "windowHint": null, "tabHint": null, "proof": null,
        "evidenceObservation": null, "invalidations": [], "valid": false, "invalidationReason": null,
        "recordedCursor": null, "invalidatedCursor": null, "createdCursor": 1, "revision": 1,
    });
    let binding: SurfaceBindingRecord = serde_json::from_value(json).expect("loads");
    assert!(binding.proof_point.is_none());
}
