import copy
import unittest
from m2_composed_duration import analyze, METHOD
from m2_socket_duration import qualify, make_profile, artifact_identity, IDENTITY_KEYS
from test_m2_socket_duration import fixture as conformance, record
from test_m2_latency import fixture as legacy, BOOT, EPOCH, TOKEN, OBSERVER, BASE

ENTITY = "00000000-0000-4000-8000-000000000010"
WITNESS = "00000000-0000-4000-8000-000000000011"


def fixture():
    raw = legacy()
    raw.update(schemaVersion=3, method=METHOD, suspendDiscontinuity=False)
    check = conformance()
    profile = make_profile(check)
    raw.update(instrumentProfile=profile, conformance=check,
        artifactIdentity=artifact_identity(check))
    raw["helperRecords"][1].update(runtimeId=EPOCH, requestObservationIds=[r["observationId"] for r in raw["helperRecords"][2]["records"]])
    raw["helperRecords"][2].update(kind="observer-duration-attempt", schemaVersion=3)
    for r in raw["helperRecords"][2]["records"]: r["phaseEntry"] = True
    context = {key: f"00000000-0000-4000-8000-{index:012x}" for index, key in enumerate(("runId", "domRuntimeId", "nativeIncarnation", "subscriptionId", "viewEpoch", "coreGeneration", "storeGeneration"), 1)}
    for row in raw["commitRecords"]:
        row["storeGeneration"] = context["storeGeneration"]
        row["projectionRequirements"] = {"toCursor": row["patchCursor"], "sessionUpserts": [{"sessionId": ENTITY, "revision": row["patchCursor"]}], "attentionUpserts": [], "tombstones": []} if row["patchCursor"] else None
    witness = {"schemaVersion": 3, "witnessId": WITNESS, "context": context, "fromCursor": "0", "cursor": "2", "fromStreamSeq": 3, "streamSeq": 5,
        "pendingHydration": 0, "visible": True, "revisions": [{"entity": "SESSION", "id": ENTITY, "revision": "2", "absent": False}]}
    clock = {"schemaVersion": 3, "clock": "CLOCK_UPTIME_RAW", "bootId": BOOT, "context": context}
    result = lambda v: {"result": {**clock, **v}, "error": None}
    raw["domRecords"] = [result({"kind": "composed-duration-start", "cursor": "0", "streamSeq": 3, "monotonicNs": str(BASE)}),
        result({"kind": "composed-dom-receipt", "witness": witness, "ordinal": 1, "monotonicNs": str(BASE + 50_000_000)}),
        result({"kind": "composed-dom-census", "totalWitnesses": 1, "failures": 0, "closed": True, "monotonicNs": str(BASE + 100_000_000)})]
    return raw


class ComposedControls(unittest.TestCase):
    def test_profile_and_artifact_changed_together_cannot_relabel_conformance(self):
        for key in IDENTITY_KEYS:
            raw = fixture()
            raw['instrumentProfile'][key] = raw['artifactIdentity'][key] = 'substituted-together'
            result = analyze(raw)
            self.assertFalse(result['metricsQualified'], key)
            self.assertTrue(all(count == 0 for metrics in result['qualifiedSampleCounts'].values() for count in metrics.values()), key)
            record('composed-calculator-identity-'+key, {'metricsQualified':result['metricsQualified'], 'qualifiedSampleCounts':result['qualifiedSampleCounts'], 'errors':result['errors']})
        raw = fixture()
        raw['instrumentProfile']['policySha256'] = raw['artifactIdentity']['policySha256'] = 'old-policy'
        self.refuse(raw)

    def refuse(self, raw, text=None):
        result = analyze(raw)
        self.assertFalse(result["normalPathPass"], result)
        self.assertNotEqual(result["verdict"], "PASS")
        if text: self.assertTrue(any(text in e for e in result["errors"]), result["errors"])
        return result

    def test_precommit_and_sum_per_observation(self):
        result = analyze(fixture())
        self.assertTrue(result["normalPathPass"], result["errors"])
        self.assertFalse(result["nativeExecution"])
        self.assertNotEqual(result["verdict"], "PASS", "synthetic controls are never native acceptance")
        observer = next(r for r in result["joinedSamples"] if r["observationId"] == OBSERVER)
        metrics = observer["upperBoundsMs"]
        self.assertAlmostEqual(metrics["commitToDomMs"], 37.001, places=3, msg="A minus PRE-COMMIT; E would omit 1 ms")
        self.assertAlmostEqual(metrics["eventToDomMs"], metrics["captureToCommitMs"] + metrics["commitToDomMs"], places=3)
        hook = result["bySource"]["claude.hook"]
        self.assertAlmostEqual(hook["eventToDomMs"]["p95"], 40.001)

    def test_forged_unbound_receipt_wrong_runtime_and_wrong_request_uuid(self):
        for mode in ("token", "runtime", "ids", "statuses", "missing"):
            raw = fixture()
            native, attempt = raw["helperRecords"][1:3]
            if mode == "token": attempt["nativeToken"] = "forged"
            if mode == "runtime": native["runtimeId"] = "wrong-runtime"
            if mode == "ids": native["requestObservationIds"][0] = "other"
            if mode == "statuses": native["receipt"]["results"][0]["status"] = "NOT_ACCEPTED"
            if mode == "missing": native["receipt"]["results"] = []
            self.refuse(raw, "no independently bound validated durable receipt")

    def test_spool_unknown_and_not_accepted_cannot_prove_durability(self):
        for status in ("LOCAL_SPOOLED", "UNKNOWN", "NOT_ACCEPTED"):
            raw = fixture()
            raw["helperRecords"][1]["receipt"]["results"][0]["status"] = status
            raw["helperRecords"][2]["records"][0]["status"] = status
            self.refuse(raw, "no independently bound validated durable receipt")

    def test_failed_attempt_retained_before_same_capture_success(self):
        raw = fixture()
        failed = copy.deepcopy(raw["helperRecords"][2])
        failed.update(nativeToken=None, endMs=14)
        for r in failed["records"]: r["status"] = "UNKNOWN"
        raw["helperRecords"].insert(2, failed)
        result = analyze(raw)
        self.assertTrue(result["normalPathPass"], result["errors"])
        self.assertEqual(next(r for r in result["joinedSamples"] if r["observationId"] == OBSERVER)["retainedAttempts"], 2)

    def test_duplicate_transaction_cannot_replace_original_commit(self):
        raw = fixture()
        retry = copy.deepcopy(raw["commitRecords"][1])
        retry["status"] = "ALREADY_COMMITTED"
        retry["commit"].update(beginNs=str(BASE + 45_000_000), endNs=str(BASE + 46_000_000))
        raw["commitRecords"].append(retry)
        result = analyze(raw)
        self.assertTrue(result["normalPathPass"], result["errors"])
        self.assertEqual(next(r for r in result["joinedSamples"] if r["observationId"] == OBSERVER)["originalCommitBeginNs"], str(BASE + 13_000_000))
        raw["commitRecords"].remove(raw["commitRecords"][1])
        self.refuse(raw, "missing/ambiguous original successful COMMIT")

    def test_stale_context_unresolved_hydration_cursor_only_and_uncovered_revisions(self):
        for mode in ("context", "hydration", "cursor", "revision", "coverage"):
            raw = fixture()
            w = raw["domRecords"][1]["result"]["witness"]
            if mode == "context": w["context"] = {**w["context"], "subscriptionId": "replacement"}
            if mode == "hydration": w["pendingHydration"] = 1
            if mode == "cursor": w["revisions"] = []
            if mode == "revision": w["revisions"][0]["revision"] = "1"
            if mode == "coverage": w["fromCursor"] = "2"
            self.refuse(raw)

    def test_missing_final_source_or_dom_census_and_lost_acknowledgement(self):
        for mode in ("source", "dom", "ack", "ordinal", "overflow", "collector"):
            raw = fixture()
            if mode == "source": raw["populationSeals"].pop()
            if mode == "dom": raw["domRecords"].pop()
            if mode == "ack": raw["domRecords"].pop(1)
            if mode == "ordinal": raw["domRecords"][1]["result"]["ordinal"] = 2
            if mode == "overflow": raw["domRecords"][2]["result"]["failures"] = 1
            if mode == "collector": raw["helperRecords"][2]["failedExports"] = 1
            self.refuse(raw)

    def test_late_ack_is_retained_and_does_not_pass_target(self):
        raw = fixture()
        raw["domRecords"][1]["result"]["monotonicNs"] = str(BASE + 400_000_000)
        raw["domRecords"][2]["result"]["monotonicNs"] = str(BASE + 500_000_000)
        result = self.refuse(raw)
        self.assertTrue(result["metricsQualified"])
        self.assertAlmostEqual(result["bySource"]["claude.observer"]["commitToDomMs"]["p95"], 387.001)

    def test_receipt_after_dom_is_permitted_and_preserves_original_capture(self):
        raw = fixture()
        raw["helperRecords"][2]["endMs"] = 80
        raw["helperRecords"][1]["monotonicNs"] = str(BASE + 79_000_000)
        result = analyze(raw)
        self.assertTrue(result["normalPathPass"], result["errors"])

    def test_clock_reset_retry_recapture_epoch_change_and_suspend_stop(self):
        for mode in ("clock", "recapture", "epoch", "suspend"):
            raw = fixture()
            if mode == "clock": raw["helperRecords"][2]["endMs"] = 1
            if mode == "recapture":
                retry = copy.deepcopy(raw["helperRecords"][2]); retry["records"][0]["capturedMs"] = 9
                raw["helperRecords"].append(retry)
            if mode == "epoch": raw["commitRecords"][1]["sourceEpoch"] = "other"
            if mode == "suspend": raw["suspendDiscontinuity"] = True
            self.refuse(raw)

    def test_changed_profile_or_uncertainty_straddling_threshold_is_refused(self):
        raw = fixture()
        raw["instrumentProfile"]["observerAdditiveMs"] = 0
        self.refuse(raw)
        raw = fixture(); raw["helperRecords"][2]["endMs"] = 109.5
        self.refuse(raw, "uncertainty determines threshold verdict")

    def test_conformance_refuses_units_coarsening_reset_discontinuity_and_wide_brackets(self):
        self.assertTrue(qualify(conformance())["qualified"])
        for mode in ("units", "coarsening", "reset", "boot", "suspend", "wide"):
            c = conformance()
            if mode == "units": c["rows"][0]["readings"][-1] *= 1000
            if mode == "coarsening": c["rows"][0]["readings"][:256] = [0] * 256
            if mode == "reset": c["rows"][1]["readings"][2] = 0
            if mode == "boot": c["bootAfter"] = "other"
            if mode == "suspend": c["powerDiscontinuity"] = True
            if mode == "wide": c["rows"][0]["endUpper"]["nativeNs"] = str(BASE + 200_000_000)
            self.assertFalse(qualify(c)["qualified"], mode)


if __name__ == "__main__": unittest.main()
