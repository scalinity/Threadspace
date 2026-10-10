//! Pure assertions for the focused F3 runner. These assertions can execute
//! portably; they do not execute or qualify a native Return.

use serde_json::{Value, json};

pub const BUDGET_MS: u64 = 2000;

pub fn all_true(value: &Value) -> bool {
    value.as_object().is_some_and(|fields| {
        !fields.is_empty() && fields.values().all(|field| field.as_bool() == Some(true))
    })
}

fn present(value: &Value) -> bool {
    value.as_str().is_some_and(|text| !text.is_empty())
}

fn inventory_matches(value: &Value, identity: &Value) -> bool {
    let matches = |rows: &Value| {
        rows.as_array().is_some_and(|rows| {
            rows.len() == 1
                && rows[0]["kind"] == "interactive"
                && rows[0]["pid"] == identity["processKey"]["pid"]
                && rows[0]["sessionId"] == identity["nativeSessionId"]
        })
    };
    value["error"].is_null()
        && value["requestStartedMs"].as_i64().is_some()
        && value["requestEndedMs"].as_i64() >= value["requestStartedMs"].as_i64()
        && matches(&value["pidRows"])
        && matches(&value["sessionRows"])
}

fn sample_matches(value: &Value, identity: &Value) -> bool {
    let sample = &value["sample"];
    value["outcome"] == "Sampled"
        && sample["pid"] == identity["processKey"]["pid"]
        && sample["startSeconds"] == identity["processKey"]["startSeconds"]
        && sample["startMicroseconds"] == identity["processKey"]["startMicroseconds"]
        && sample["executable"] == identity["executable"]
        && sample["controllingDevice"] == identity["rdev"]
}

pub fn same_identity(left: &Value, right: &Value) -> bool {
    left["qualified"] == true
        && right["qualified"] == true
        && present(&left["nativeSessionId"])
        && present(&left["processKey"]["endpointId"])
        && present(&left["processKey"]["bootId"])
        && present(&left["processKey"]["startSeconds"])
        && left["processKey"]["pid"]
            .as_u64()
            .is_some_and(|pid| pid > 1)
        && left["nativeSessionId"] == right["nativeSessionId"]
        && left["processKey"] == right["processKey"]
        && present(&left["executable"])
        && left["executable"] == right["executable"]
        && left["rdev"].as_u64().is_some()
        && left["rdev"] == right["rdev"]
        && present(&left["tty"])
        && left["tty"] == right["tty"]
        && left["runningVersion"] == "2.1.295"
        && right["runningVersion"] == "2.1.295"
}

/// Every meaningful positive assertion has an explicit witness. Missing or
/// failed readbacks are false, including when the product safely times out.
pub fn positive_checks(case: &Value) -> Value {
    let expected = &case["expected"];
    let identity = &case["preconditions"]["native"];
    let route = &case["route"];
    let result = &route["result"];
    let evidence = &result["evidence"];
    let focus = &evidence["focus"];
    let independent = &case["independent"];
    let terminal = &evidence["terminal"];
    let unique_tab = terminal["matches"].as_array().is_some_and(|tabs| {
        tabs.len() == 1
            && tabs[0]["windowId"] == expected["windowId"]
            && tabs[0]["tty"] == identity["tty"]
            && tabs[0]["rdev"] == identity["rdev"]
    });
    let selected = independent["selection"]["selected"]
        .get(expected["windowId"].to_string())
        .unwrap_or(&Value::Null);
    let revision = &evidence["bindingRevisionLoaded"];
    let required_phases = [
        "pre-lookup sample",
        "provider lookup + terminal enumeration",
        "post-lookup sample",
        "surface join",
        "focus + readback",
        "frontmost + readback stat",
        "post-focus revalidation",
    ];
    let phases_complete = evidence["phases"].as_array().is_some_and(|phases| {
        required_phases.iter().all(|wanted| {
            phases
                .iter()
                .any(|phase| phase["phase"] == *wanted && phase["elapsedMs"].as_u64().is_some())
        })
    });
    json!({
        "preconditionsEstablished": all_true(&case["preconditions"]["checks"]),
        "rawMinimizedPrecondition": case["preconditions"]["readback"]["targetState"]["ok"] == true
            && case["preconditions"]["readback"]["targetState"]["miniaturized"] == true,
        "rawOtherForegroundPrecondition": case["preconditions"]["readback"]["selection"]["error"].is_null()
            && expected["otherWindowId"].as_i64().is_some()
            && expected["otherWindowId"] != expected["windowId"]
            && case["preconditions"]["readback"]["selection"]["front"] == expected["otherWindowId"]
            && case["preconditions"]["readback"]["selection"]["selected"].get(expected["otherWindowId"].to_string()) == Some(&expected["otherTty"])
            && present(&expected["otherTty"])
            && case["preconditions"]["readback"]["frontmostBundle"] == "com.apple.Terminal",
        "sameOriginalNativeIdentity": same_identity(&expected["native"], identity),
        "sameIndependentPostReturnIdentity": same_identity(identity, &case["independentCurrent"]),
        "explicitReturnIssued": case["explicitReturn"] == true,
        "matchingRequest": present(&route["requestId"]) && result["requestId"] == route["requestId"],
        "matchingCanonicalSession": present(&expected["sessionId"]) && result["sessionId"] == expected["sessionId"],
        "exactCurrentSuccess": result["surfaceResult"] == "EXACT_NATIVE_SURFACE"
            && result["sessionVerification"] == "CURRENT_NATIVE_REVALIDATED"
            && result["inputReadiness"] == "FOREGROUND_COMPATIBLE" && result["reasonCode"] == "OK",
        "withinOriginalRouteDeadline": result["latencyMs"].as_u64().is_some_and(|ms| ms <= BUDGET_MS),
        "requestThroughReceiptWithinBudget": route["harnessElapsedMs"].as_u64().is_some_and(|ms| ms <= BUDGET_MS),
        "sameProcessKeyAndImage": evidence["processKey"] == identity["processKey"]
            && evidence["boundExecutable"] == identity["executable"] && evidence["boundDevice"] == identity["rdev"],
        "sameNativeSession": evidence["nativeSessionId"] == identity["nativeSessionId"],
        "preLookupKernelProof": sample_matches(&evidence["preLookupSample"], identity),
        "postLookupKernelProof": sample_matches(&evidence["postLookupSample"], identity),
        "postFocusKernelProof": sample_matches(&evidence["postFocusSample"], identity),
        "freshLookupProof": inventory_matches(&evidence["lookup"], identity),
        "postFocusLookupProof": inventory_matches(&evidence["postFocusLookup"], identity),
        "stableBindingRevision": present(revision) && evidence["bindingRevisionBeforeFocus"] == *revision
            && evidence["bindingRevisionAfterFocus"] == *revision && result["validatedBindingRevision"] == *revision,
        "uniqueTerminalDeviceMatch": unique_tab && terminal["error"].is_null(),
        "stableTerminalIncarnation": !terminal["generationBefore"].is_null()
            && terminal["generationBefore"] == terminal["generationAfter"],
        "nativeFocusAndReadback": result["focusPerformed"] == true && focus["outcome"] == "FOCUSED"
            && focus["targetWindowId"] == expected["windowId"] && focus["frontWindowId"] == expected["windowId"]
            && focus["readbackTty"] == identity["tty"] && focus["readbackRdev"] == identity["rdev"]
            && focus["targetWindowFrontmost"] == true && focus["targetTabSelected"] == true
            && focus["frontmostApplication"]["bundleIdentifier"] == "com.apple.Terminal" && focus["error"].is_null(),
        "independentTargetRestored": independent["targetState"]["ok"] == true
            && independent["targetState"]["miniaturized"] == false,
        "independentExactFrontWindowAndTab": independent["selection"]["error"].is_null()
            && independent["selection"]["front"] == expected["windowId"] && *selected == identity["tty"],
        "independentTerminalFrontmost": independent["frontmostBundle"] == "com.apple.Terminal",
        "noUnrelatedSelectionChanges": case["sideEffects"]["unrelatedSelectionChanges"].as_array().is_some_and(Vec::is_empty),
        "completeRoutePhaseRecords": phases_complete,
    })
}

/// A changed or disappeared unrelated window is retained as a difference;
/// unreadable inventories stay unknown instead of becoming an empty list.
pub fn unrelated_changes(before: &Value, after: &Value, owned: &[i64]) -> Value {
    let (Some(before), Some(after)) = (
        before["selected"].as_object(),
        after["selected"].as_object(),
    ) else {
        return Value::Null;
    };
    if before.is_empty() || after.is_empty() {
        return Value::Null;
    }
    json!(
        before
            .iter()
            .filter_map(|(id, tty)| {
                (!owned.iter().any(|owned| owned.to_string() == *id) && after.get(id) != Some(tty))
                    .then(|| id.clone())
            })
            .collect::<Vec<_>>()
    )
}

pub fn negative_checks(case: &Value) -> Value {
    let result = &case["route"]["result"];
    let before = &case["before"];
    let after = &case["after"];
    json!({
        "unknownSessionVerifiedAbsent": case["sessionAbsent"] == true,
        "explicitReturnIssued": case["explicitReturn"] == true,
        "honestUnavailable": result["surfaceResult"] == "UNAVAILABLE" && result["reasonCode"] == "SESSION_NOT_FOUND",
        "noFocusAppleEventReported": result["focusPerformed"] == false,
        "boundedRequest": result["latencyMs"].as_u64().is_some_and(|ms| ms <= BUDGET_MS)
            && case["route"]["harnessElapsedMs"].as_u64().is_some_and(|ms| ms <= BUDGET_MS),
        "independentFrontmostUnchanged": present(&before["frontmostBundle"])
            && before["frontmostBundle"] == after["frontmostBundle"],
        "independentWindowAndSelectionsUnchanged": before["selection"]["error"].is_null()
            && after["selection"]["error"].is_null() && before["selection"]["front"].as_i64().is_some()
            && before["selection"]["front"] == after["selection"]["front"]
            && before["selection"]["selected"].as_object().is_some_and(|v| !v.is_empty())
            && before["selection"]["selected"] == after["selection"]["selected"],
        "targetMinimizedStateUnchanged": before["targetState"]["ok"] == true && after["targetState"]["ok"] == true
            && before["targetState"]["miniaturized"] == after["targetState"]["miniaturized"],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete_positive() -> Value {
        let identity = json!({ "qualified": true, "nativeSessionId": "native-A", "runningVersion": "2.1.295",
            "processKey": { "endpointId": "endpoint", "bootId": "boot", "pid": 42, "startSeconds": "100", "startMicroseconds": 1 },
            "executable": "/fixture/claude/versions/2.1.295#1:2", "tty": "/dev/ttys001", "rdev": 123 });
        let inventory = json!({ "requestStartedMs": 1, "requestEndedMs": 2,
            "pidRows": [{ "kind": "interactive", "pid": 42, "sessionId": "native-A" }],
            "sessionRows": [{ "kind": "interactive", "pid": 42, "sessionId": "native-A" }] });
        let sample = json!({ "outcome": "Sampled", "sample": { "pid": 42, "startSeconds": "100", "startMicroseconds": 1,
            "executable": identity["executable"], "controllingDevice": 123 } });
        let phases = [
            "pre-lookup sample",
            "provider lookup + terminal enumeration",
            "post-lookup sample",
            "surface join",
            "focus + readback",
            "frontmost + readback stat",
            "post-focus revalidation",
        ];
        let evidence = json!({ "processKey": identity["processKey"], "boundExecutable": identity["executable"], "boundDevice": 123,
            "nativeSessionId": "native-A", "preLookupSample": sample, "postLookupSample": sample, "postFocusSample": sample,
            "lookup": inventory, "postFocusLookup": inventory,
            "bindingRevisionLoaded": "12", "bindingRevisionBeforeFocus": "12", "bindingRevisionAfterFocus": "12",
            "terminal": { "generationBefore": { "pid": 5, "startSeconds": "80" }, "generationAfter": { "pid": 5, "startSeconds": "80" },
                "matches": [{ "windowId": 10, "tty": "/dev/ttys001", "rdev": 123 }] },
            "focus": { "outcome": "FOCUSED", "targetWindowId": 10, "frontWindowId": 10, "readbackTty": "/dev/ttys001",
                "readbackRdev": 123, "targetWindowFrontmost": true, "targetTabSelected": true,
                "frontmostApplication": { "bundleIdentifier": "com.apple.Terminal" } },
            "phases": phases.map(|phase| json!({ "phase": phase, "elapsedMs": 1 })),
        });
        let result = json!({ "requestId": "request", "sessionId": "canonical-A", "surfaceResult": "EXACT_NATIVE_SURFACE",
            "sessionVerification": "CURRENT_NATIVE_REVALIDATED", "inputReadiness": "FOREGROUND_COMPATIBLE",
            "reasonCode": "OK", "latencyMs": 1998, "focusPerformed": true, "validatedBindingRevision": "12", "evidence": evidence });
        json!({ "expected": { "sessionId": "canonical-A", "windowId": 10, "native": identity,
                "otherWindowId": 11, "otherTty": "/dev/ttys002" },
            "preconditions": { "native": identity, "checks": { "minimized": true, "otherForeground": true, "owned": true },
                "readback": { "targetState": { "ok": true, "miniaturized": true }, "frontmostBundle": "com.apple.Terminal",
                    "selection": { "front": 11, "selected": { "11": "/dev/ttys002" } } } },
            "explicitReturn": true, "independentCurrent": identity,
            "route": { "requestId": "request", "harnessElapsedMs": 1999, "result": result },
            "independent": { "selection": { "front": 10, "selected": { "10": "/dev/ttys001" } },
                "frontmostBundle": "com.apple.Terminal", "targetState": { "ok": true, "miniaturized": false } },
            "sideEffects": { "unrelatedSelectionChanges": [] },
        })
    }

    #[test]
    fn complete_synthetic_witness_passes_but_not_as_native_qualification() {
        let checks = positive_checks(&complete_positive());
        assert!(all_true(&checks), "{checks:#}");
    }

    #[test]
    fn each_mandatory_proof_and_deadline_controls_the_positive_verdict() {
        let original = complete_positive();
        for pointer in [
            "/preconditions/checks/minimized",
            "/preconditions/readback/targetState/miniaturized",
            "/preconditions/readback/selection/front",
            "/route/result/evidence/postFocusSample",
            "/route/result/evidence/postFocusLookup",
            "/route/result/evidence/terminal/matches",
            "/route/result/evidence/focus/readbackTty",
            "/independent/selection",
            "/independentCurrent/processKey",
            "/route/result/evidence/phases",
            "/sideEffects/unrelatedSelectionChanges",
        ] {
            let mut case = original.clone();
            *case.pointer_mut(pointer).expect("fixture field") = Value::Null;
            assert!(
                !all_true(&positive_checks(&case)),
                "missing proof {pointer} passed"
            );
        }
        for pointer in ["/route/result/latencyMs", "/route/harnessElapsedMs"] {
            let mut case = original.clone();
            *case.pointer_mut(pointer).expect("deadline field") = json!(2001);
            assert!(
                !all_true(&positive_checks(&case)),
                "late field {pointer} passed"
            );
        }
        let mut wrong = original;
        wrong["independent"]["selection"]["selected"]["10"] = json!("/dev/ttys999");
        assert!(!all_true(&positive_checks(&wrong)));
    }

    #[test]
    fn absent_evidence_never_passes() {
        assert!(!all_true(&positive_checks(&json!({}))));
        assert!(!all_true(&negative_checks(&json!({}))));
        assert!(!all_true(&json!({})));
        assert_eq!(unrelated_changes(&json!({}), &json!({}), &[]), Value::Null);
    }

    #[test]
    fn timeout_with_restoration_and_wrong_target_false_is_not_positive() {
        let case = json!({
            "route": { "requestId": "request", "harnessElapsedMs": 2004, "result": {
                "requestId": "request", "surfaceResult": "UNAVAILABLE", "reasonCode": "TIMEOUT",
                "sessionVerification": "NATIVE_BOUND_LAST_KNOWN", "latencyMs": 2003,
                "focusPerformed": true,
            }},
            "wrongTarget": false,
            "independent": { "targetState": { "ok": true, "miniaturized": false } },
        });
        let checks = positive_checks(&case);
        assert!(!all_true(&checks));
        assert_eq!(checks["exactCurrentSuccess"], false);
        assert_eq!(checks["withinOriginalRouteDeadline"], false);
        assert_eq!(checks["independentTargetRestored"], true);
        assert_eq!(checks["independentExactFrontWindowAndTab"], false);
    }

    #[test]
    fn unknown_negative_does_not_conceal_focus_or_missing_readback() {
        let mut case = json!({ "sessionAbsent": true, "explicitReturn": true,
            "route": { "harnessElapsedMs": 7, "result": { "surfaceResult": "UNAVAILABLE",
                "reasonCode": "SESSION_NOT_FOUND", "focusPerformed": false, "latencyMs": 5 } },
            "before": { "frontmostBundle": "com.apple.Terminal", "selection": { "front": 2, "selected": { "2": "/dev/ttys002" } },
                "targetState": { "ok": true, "miniaturized": false } },
            "after": { "frontmostBundle": "com.apple.Terminal", "selection": { "front": 2, "selected": { "2": "/dev/ttys002" } },
                "targetState": { "ok": true, "miniaturized": false } },
        });
        assert!(all_true(&negative_checks(&case)));
        case["route"]["result"]["focusPerformed"] = json!(true);
        assert!(!all_true(&negative_checks(&case)));
        case["route"]["result"]["focusPerformed"] = json!(false);
        case["after"]["selection"] = Value::Null;
        assert!(!all_true(&negative_checks(&case)));
    }

    #[test]
    fn changes_to_unowned_windows_are_visible_on_failure_too() {
        let before = json!({ "selected": { "1": "target", "2": "spare", "3": "owner" } });
        let after = json!({ "selected": { "1": "target", "2": "spare", "3": "other" } });
        assert_eq!(unrelated_changes(&before, &after, &[1, 2]), json!(["3"]));
    }
}
