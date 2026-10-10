import copy
import hashlib
import unittest

from m2_latency import analyze, percentiles

BOOT = "qualification-boot"
EPOCH = "11111111-1111-4111-8111-111111111111"
HOOK = "22222222-2222-4222-8222-222222222222"
OBSERVER = "33333333-3333-4333-8333-333333333333"
METADATA = "44444444-4444-4444-8444-444444444444"
TOKEN = "55555555-5555-4555-8555-555555555555"
BASE = 1_000_000_000


def fixture():
    clock = lambda ms: {"clockQuality": "LOCAL_MONOTONIC", "bootId": BOOT, "monotonicNs": str(BASE + ms * 1_000_000)}
    commit = lambda observation_id, source, begin, end, cursor, patch: {
        "event": "M2_COMMITTED_MEASUREMENT", "observationId": observation_id, "source": source, "bootId": BOOT,
        "cursor": cursor, "patchCursor": patch, "status": "COMMITTED", "capture": clock(10),
        "storeGeneration": "store", "sourceEpoch": BOOT if source == "claude.hook" else EPOCH,
        "commit": {"boundary": "SQLITE_COMMIT_CALL", "clock": "CLOCK_UPTIME_RAW", "beginNs": str(BASE + begin * 1_000_000), "endNs": str(BASE + end * 1_000_000)}}
    return {
        "schemaVersion": 2, "executionKind": "SYNTHETIC_UNIT_TEST", "bootId": BOOT,
        "clockQualification": {"nativeClock": "CLOCK_UPTIME_RAW", "qualified": True, "maximumRateErrorPpm": 0, "precisionNs": 1000, "evidence": "SYNTHETIC_FIXED_RATE_CLOCK_ORACLE"},
        "helperRecords": [
            {"kind": "hook-capture", "observationId": HOOK, "source": "claude.hook", "sourceEpoch": BOOT, "capture": clock(10)},
            {"kind": "helper-receipt", "token": TOKEN, "clock": "CLOCK_UPTIME_RAW", "bootId": BOOT, "monotonicNs": str(BASE + 14_000_000),
             "receipt": {"receiptVersion": 1, "results": [{"observationId": OBSERVER, "status": "COMMITTED"}, {"observationId": METADATA, "status": "COMMITTED"}]}},
            {"kind": "observer-clock-bracket", "schemaVersion": 1, "runtimeId": EPOCH, "clock": "performance.now", "beginMs": 12, "endMs": 16,
             "nativeToken": TOKEN, "totalCaptured": 2, "overflow": 0, "failedExports": 0,
             "records": [{"observationId": OBSERVER, "capturedMs": 10, "status": "COMMITTED"}, {"observationId": METADATA, "capturedMs": 11, "status": "COMMITTED"}]}],
        "commitRecords": [commit(HOOK, "claude.hook", 12, 13, "1", "1"), commit(OBSERVER, "claude.observer", 13, 14, "2", "2"), commit(METADATA, "claude.observer", 13, 14, "3", None)],
        "domPages": [{"schemaVersion": 2, "runtimeId": "view-runtime", "clock": "performance.now", "context": {"phase": "live", "viewEpoch": "view", "coreGeneration": "core", "storeGeneration": "store"},
                      "totalApplied": 2, "retainedMarks": 2, "overflow": 0, "invalid": 0, "contextChanged": False, "offset": 0, "nextOffset": None,
                      "marks": [{"cursor": "1", "domCursor": "1", "appliedMonotonicMs": 15, "domMonotonicMs": 20, "hydratedVisible": True},
                                {"cursor": "2", "domCursor": "2", "appliedMonotonicMs": 24, "domMonotonicMs": 30, "hydratedVisible": True}]}],
        "populationSeals": [{"source": source, "boundary": "CLOSED_CAPTURE_WINDOW", "sealed": True, "bootId": BOOT,
                             "totalCaptured": len(ids), "observationIdsSha256": hashlib.sha256("\n".join(sorted(ids)).encode()).hexdigest(),
                             "telemetryFailures": 0, "runtimeIds": [EPOCH] if source == "claude.observer" else [], "evidence": "SYNTHETIC_CENSUS"}
                            for source, ids in (("claude.hook", [HOOK]), ("claude.observer", [OBSERVER, METADATA]))],
        "uiCalibrations": [{"bootId": BOOT, "clock": "CLOCK_UPTIME_RAW", "nativeBeforeNs": str(BASE + ms * 1_000_000), "nativeAfterNs": str(BASE + ms * 1_000_000 + 1000),
                            "reply": {"runtimeId": "view-runtime", "clock": "performance.now", "monotonicMs": ms}} for ms in (0, 100)],
    }


class LatencyAcceptanceTests(unittest.TestCase):
    def assert_incomplete(self, raw):
        result = analyze(raw)
        self.assertFalse(result["normalPathPass"], result)
        self.assertTrue(result["errors"])

    def test_true_commit_and_actual_dom_use_conservative_bounds_with_full_denominators(self):
        result = analyze(fixture())
        self.assertTrue(result["normalPathPass"], result["errors"])
        self.assertFalse(result["nativeExecution"], "calculation examples are never native execution")
        self.assertEqual(result["verdict"], "INCOMPLETE_OR_FAIL")
        hook, observer = result["bySource"]["claude.hook"], result["bySource"]["claude.observer"]
        self.assertEqual(hook["captureToCommitMs"]["p95"], 3)
        self.assertAlmostEqual(observer["captureToCommitMs"]["p95"], 6.001)
        self.assertEqual(observer["population"]["totalCaptured"], 2)
        self.assertEqual(observer["population"]["noProjectionChange"], 1)
        self.assertEqual(observer["population"]["matchedDom"], 1)
        self.assertEqual(len(result["exclusions"]), 1)

    def test_pretransaction_receipt_cannot_be_labeled_commit(self):
        raw = fixture()
        raw["commitRecords"][0]["commit"]["boundary"] = "WRITER_RECEIVED"
        raw["commitRecords"][0]["received_wall_ms"] = 1
        self.assert_incomplete(raw)

    def test_observer_is_mandatory_even_when_hook_passes(self):
        raw = fixture()
        raw["helperRecords"] = raw["helperRecords"][:1]
        raw["commitRecords"] = raw["commitRecords"][:1]
        self.assert_incomplete(raw)

    def test_observer_threshold_is_not_ignored(self):
        raw = fixture()
        raw["helperRecords"][2]["records"][0]["capturedMs"] = 0
        raw["commitRecords"][1]["commit"]["endNs"] = str(BASE + 200_000_000)
        self.assert_incomplete(raw)

    def test_unmatched_dom_is_counted_instead_of_filtered(self):
        raw = fixture()
        raw["domPages"][0]["marks"].pop()
        raw["domPages"][0]["retainedMarks"] = raw["domPages"][0]["totalApplied"] = 1
        result = analyze(raw)
        self.assertFalse(result["normalPathPass"])
        self.assertEqual(result["bySource"]["claude.observer"]["population"]["unmatchedDom"], 1)

    def test_coalesced_commit_cursors_join_the_next_actual_render(self):
        raw = fixture()
        raw["domPages"][0]["marks"].pop(0)
        raw["domPages"][0]["retainedMarks"] = raw["domPages"][0]["totalApplied"] = 1
        result = analyze(raw)
        self.assertTrue(result["normalPathPass"], result["errors"])
        self.assertEqual(result["bySource"]["claude.hook"]["population"]["matchedDom"], 1)

    def test_received_or_store_applied_is_not_dom_applied(self):
        raw = fixture()
        raw["domPages"][0]["marks"][0]["domMonotonicMs"] = None
        self.assert_incomplete(raw)

    def test_unqualified_and_wrong_boot_clock_profiles_fail(self):
        for mutation in ("unqualified", "boot", "wall"):
            raw = fixture()
            if mutation == "unqualified": raw["clockQualification"]["qualified"] = False
            elif mutation == "boot": raw["uiCalibrations"][0]["bootId"] = "other-boot"
            else: raw["clockQualification"]["nativeClock"] = "Date.now"
            self.assert_incomplete(raw)

    def test_unqualified_diagnostics_never_report_qualified_sample_counts(self):
        raw = fixture()
        raw["clockQualification"].update(qualified=False, maximumRateErrorPpm=None, precisionNs=None)
        result = analyze(raw)
        self.assertFalse(result["metricsQualified"])
        self.assertIn("UNQUALIFIED", result["calculation"])
        self.assertEqual(result["diagnosticClockFallback"], {"maximumRateErrorPpm": 0, "precisionNs": 0, "certified": False})
        self.assertTrue(all(count == 0 for metrics in result["qualifiedSampleCounts"].values() for count in metrics.values()))
        self.assertEqual(result["bySource"]["claude.hook"]["captureToCommitMs"]["n"], 1, "retain every diagnostic sample")

    def test_valid_slow_measurement_is_qualified_even_when_target_fails(self):
        raw = fixture()
        raw["commitRecords"][0]["commit"]["endNs"] = str(BASE + 120_000_000)
        result = analyze(raw)
        self.assertTrue(result["metricsQualified"], result["errors"])
        self.assertFalse(result["normalPathPass"])
        self.assertEqual(result["qualifiedSampleCounts"]["claude.hook"]["captureToCommitMs"], 1)

    def test_incompatible_clock_rates_and_unbracketed_dom_fail(self):
        for mutation in ("jump", "outside"):
            raw = fixture()
            if mutation == "jump": raw["uiCalibrations"][1]["nativeBeforeNs"] = str(BASE + 500_000_000)
            else: raw["uiCalibrations"].pop()
            self.assert_incomplete(raw)

    def test_forged_process_result_token_cannot_calibrate_without_independent_record(self):
        raw = fixture()
        raw["helperRecords"][2]["nativeToken"] = "not-retained"
        self.assert_incomplete(raw)

    def test_native_receipt_must_match_the_same_observation_ids_and_statuses(self):
        for mutation in ("id", "status"):
            raw = fixture()
            record = raw["helperRecords"][1]["receipt"]["results"][0]
            record["observationId" if mutation == "id" else "status"] = "wrong"
            self.assert_incomplete(raw)

    def test_missing_observer_records_and_overflow_fail_population_accounting(self):
        for field in ("totalCaptured", "overflow", "failedExports"):
            raw = fixture()
            raw["helperRecords"][2][field] += 1
            self.assert_incomplete(raw)

    def test_dom_overflow_and_epoch_changes_fail(self):
        for field, value in (("overflow", 1), ("contextChanged", True), ("retainedMarks", 3)):
            raw = fixture()
            raw["domPages"][0][field] = value
            self.assert_incomplete(raw)

    def test_durable_commit_without_capture_candidate_is_not_silently_excluded(self):
        raw = fixture()
        raw["helperRecords"].pop(0)
        self.assert_incomplete(raw)

    def test_missing_commit_does_not_pass_from_helper_receipt_alone(self):
        raw = fixture()
        raw["commitRecords"].pop(1)
        self.assert_incomplete(raw)

    def test_uuid_retry_counts_attempt_but_not_another_canonical_capture(self):
        raw = fixture()
        retry = copy.deepcopy(raw["commitRecords"][1])
        retry["status"] = "ALREADY_COMMITTED"
        raw["commitRecords"].append(retry)
        raw["helperRecords"].append(copy.deepcopy(raw["helperRecords"][2]))
        result = analyze(raw)
        self.assertTrue(result["normalPathPass"], result["errors"])
        self.assertEqual(result["bySource"]["claude.observer"]["population"]["totalCaptured"], 2)
        self.assertEqual(result["bySource"]["claude.observer"]["population"]["deduplicatedAttempts"], 1)

    def test_nearest_rank_p95_keeps_slow_samples(self):
        values = [1] * 18 + [101, 200]
        self.assertEqual(percentiles(values), {"n": 20, "p50": 1, "p95": 101, "max": 200})

    def test_final_export_tail_cannot_disappear_behind_prior_success(self):
        raw = fixture()
        raw["populationSeals"] = []
        self.assert_incomplete(raw)
        raw = fixture()
        raw["helperRecords"][1]["receipt"]["results"].pop()
        raw["helperRecords"][2]["records"].pop()
        raw["helperRecords"][2]["totalCaptured"] = 1
        raw["commitRecords"].pop()
        self.assert_incomplete(raw)

    def test_wrong_store_or_observer_epoch_cannot_join_coincident_cursor(self):
        for field in ("storeGeneration", "sourceEpoch"):
            raw = fixture()
            raw["commitRecords"][1][field] = "other"
            self.assert_incomplete(raw)

    def test_missing_clock_rate_or_null_view_identities_is_incomplete(self):
        raw = fixture()
        raw["clockQualification"].pop("maximumRateErrorPpm")
        self.assert_incomplete(raw)
        for field in ("viewEpoch", "coreGeneration", "storeGeneration"):
            raw = fixture()
            raw["domPages"][0]["context"][field] = None
            self.assert_incomplete(raw)

    def test_collector_errors_and_unacknowledged_seals_are_not_ignored(self):
        raw = fixture()
        raw["collectorErrors"] = ["capture helper could not persist final census"]
        self.assert_incomplete(raw)
        raw = fixture()
        raw["populationSeals"][1]["sealed"] = False
        self.assert_incomplete(raw)


if __name__ == "__main__":
    unittest.main()
