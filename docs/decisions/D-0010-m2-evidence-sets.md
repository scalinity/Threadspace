# D-0010 — Reducer 3: evidence sets for the three M1 last-observation fields, host-read lifecycle evidence

**Status:** PROPOSED for M2 independent review.
**Affects:** SPEC §5.2, §5.5, §6.1, §7.3, §9.3, §11.4; D-0005 (reload rule), D-0007 §6 and §10; MILESTONES M2 exit gate.

## Context

D-0007 §10 left three canonical fields as the latest arrival: an execution's attach mode and presence, the human-follow-up frontier, and a session's observer link state. M2's producers (the observer mod, the companion's ordered inventory, conventional hooks beside the mod) can emit competing valid observations of each, so last-arrival-wins would make canonical state depend on delivery order. The same review found two more arrival-dependent values on the follow-up path: an input's origin, submission point and active turn were each overwritten by every submission report, and the frontier only ever advanced, so an acceptance followed by a rejection left it advanced while the reverse order did not.

D-0005 requires that a reloaded observer's session identity, learned through the interceptable `$.session.id()`, stays lower-tier until separate native proof restores authority, and that nothing is reattributed to the currently active session.

## Decisions

1. **Reducer version 3.** Reduction semantics and the checkpoint representation change, so `REDUCER_VERSION` is 3.

2. **Evidence sets.** Each report is kept as an observation with its observation ID, fact index, causal point and provenance:
   - `ExecutionRecord.attachments` (attach mode, presence, runtime ID, device);
   - `SessionRecord.links` (link state, provider process, whether the observer's profile is qualified and its version);
   - `InputRecord.submissions` (origin, original submission point, active turn).

   The displayed value is derived each pass from the reports no other report causally follows (`causal::compare`). When those disagree the result is conservative and flagged: the lowest execution mode, the least live presence (`DETACHED > PARKED > LIVE`), no runtime ID or device, an `UNCLASSIFIED` origin (which never qualifies a follow-up), and the most degraded link (`DISCONNECTED > CONFLICT > STALE > UNKNOWN > CURRENT`). `attachmentConflict` and `linkConflict` make the disagreement visible. A record with no report keeps the value an M0 baseline gave it; any report supersedes that value, since the baseline precedes every journal fact.

3. **The frontier is re-derived, not advanced.** A session's frontiers are recomputed from every input it holds whenever one changes: one per (actor, order domain) of the inputs with verified accepted human provenance and no rejection. A late rejection withdraws what an earlier acceptance advanced, and a frontier no input supports is removed.

4. **Host-read lifecycle evidence.** A fact's provenance can be `HOST_READ`: a provider event whose session attribution rests on a middleware-interceptable host read. That is a reloaded observer before proof, or a non-engine dispatch. A host-read turn outcome is retained in `TurnRecord.pendingOutcomes` and applied only once kernel/inventory evidence shows its provider process running the turn's Session. That means an execution of that ProcessKey attached by anything other than a host-read report. Corroboration only grows, so the result does not depend on which arrives first. An outcome from any other process is retained and never applied. The session's `observerTier` is `NATIVE` (qualified, engine-stamped identity), `RESTORED` (qualified, host-read identity, corroborated process) or `LOWER_TIER` (anything else, including an unqualified profile). An unqualified profile's adapter emits no lifecycle outcome at all.

5. **Upgrade from reducers 1 and 2.**
   - A checkpoint written before reducer 3 holds derived values, not the reports they came from. Before the existing upgrade, every fact at or before the checkpoint's cursor is read back into the sets through the same `record_evidence` the reducer uses, so the sets equal what reducing those facts records.
   - After the re-derivation, a record whose materialized row the upgrade left as committed keeps its committed revision. One whose row it changed takes the upgrade's cursor.
   - The upgraded state is therefore the same, to the state hash, wherever the earlier checkpoint sits. Attention and outbox keep D-0007 §6's handling; a binding keeps its proof revision.
   - A link report written before reducer 3 is read as reducer 2 treated every report: qualified.

6. **The mod's `session.attach`/`detach` is not an execution attach.** It has no activation identity, so mapping it onto `ExecutionAttached` would create a second execution per Session and turn exact Returns into `MULTIPLE_ATTACHMENTS` refusals. It stays journaled metadata.

## Consequences

- Equivalent admissible histories converge in the semantic projection (which now includes the conflict flags, the observer tier and pending outcomes). Exact replay, checkpoint-plus-suffix replay and restart reproduce the full state hash. Arrival-ordered bookkeeping (revisions, created cursors) is excluded from cross-order comparison as D-0007 §3 states.
- An upgraded reducer-2 store can resolve differently from reducer 2 where reducer 2 depended on arrival order. The reducer-2 fixtures (`fixtures/m2/reducer-2-store`) record such cases: a rejected follow-up no longer resolves the earlier output, and its suppressed intent re-arms HELD, never live.
- Production stays on reducer 2 until M2 is accepted. Deploying an M2 build upgrades a store irreversibly for reducer-2 builds, so it follows the controlled backup/rehearsal procedure used for reducer 2.
