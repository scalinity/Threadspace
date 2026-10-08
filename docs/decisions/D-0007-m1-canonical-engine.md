# D-0007 — M1 canonical engine: evidence-set reduction, derived identities and scoped limits

**Status:** PROPOSED with the M1 candidate; for acceptance by the M1 independent review.
**Affects:** SPEC §5.1–§5.5 (admission, identity, reduction, semantic equality), §6.1 (turn state), §7.1–§7.6 (attention, follow-up, outbox), §8.2–§8.5 (capture, spool), §9.2–§9.4 (projections, checkpoints, migration), §11.2/§11.3 (Claude classic hooks, inventory); MILESTONES M1, M2, M5.

## Evidence

`evidence/M1/` (`manifest.json`, `README.md`, `reducer-invariants.md`) and the code it cites: `crates/state-engine`, `crates/journal/src/{canonical,materialize,baseline}.rs`, `crates/relay`, `crates/provider-claude/src/hooks.rs`.

## Decisions

1. **Reduction is over evidence sets.** A fact adds evidence to the records it references (a turn's outcome set, an execution's end reasons, a wait scope's positive witnesses and clear barriers, an item's resolution causes); every displayed state is re-derived from that evidence in a fixed dependency order. No field is "whatever arrived last" where arrival order could differ between valid deliveries. This is how valid permutations converge (INV-14) and why a terminal turn, an ENDED execution and a lost binding cannot regress.
2. **Identity allocation is an admission step; derived records are name-based.** Admission allocates a random UUID only for a never-seen native key, recorded in `identity_assignments` with the cursor that allocated it. Records the reducer derives (attention items, wait episodes, outbox intents) get RFC 9562 version-8 UUIDs from SHA-256 of their canonical scope, looked up by scope first, so exact replay needs no randomness and a migrated M0 item keeps its ID.
3. **Semantic equality excludes presentation and delivery history.** Beyond SPEC §5.5's list (allocated UUIDs, cursors and revisions, receipt and creation times, allocation-order metadata such as activation numbers), the comparison excludes display names, summaries, route records and surface status (presentation or diagnostics), and compares the outbox by its still-eligible intents only: a suppressed intent depends on when resolving evidence arrived (SPEC §7.6 already accepts that a banner submitted before late evidence may have appeared).
4. **Aggregate waits partition by comparable clears.** A positive witness belongs to the episode numbered by the comparable clears before it; it is cleared by a comparable clear at or after it. A positive incomparable with a clear keeps its episode uncertain and unresolved. Native wait resolution is re-derived from current evidence (so late incomparable evidence can withhold it); owner resolutions are never removed.
5. **In-place `exec` invalidates by current image.** A process keeps every observed image with its causal point; a binding is invalidated as EXECUTABLE_REPLACED only when the current image (the observation every other precedes) is determinable and differs from the image the binding was proven with. Unordered observations assume no current image and leave invalidation to explicit ends, as the M0B store did, so a re-exec'd provider can requalify.
6. **M0 projections are promoted, not paralleled.** The M0 tables are the reducer's materialized projections (same columns plus new ones); a store written before M1 gets a deterministic `M0_BASELINE` checkpoint built from its rows. Every projection row has one builder, used to write it and to hash it, so `tables == state` is checkable at any time.
7. **Claude conventional hooks map conservatively (CLAUDE_CLASSIC_LIMITED).** Identity, unaccepted submissions, Stop/SubagentStop as response boundaries, tool phases and subordinate identification; never a turn outcome or execution end (conventional hooks carry no native turn identity). SessionEnd stays identity-only until M2/M3 match activations.
8. **Two deliveries are deliberately later milestones.** Canonical attention creates durable outbox intents in the same transaction, but OS submission of those intents is M5's notification product; Claude inventory `waiting`/`waitingFor` remain display metadata until M2's inventory integration maps them to session-scoped wait episodes.
9. **Capture budgets and spool bounds are enforced structurally.** The hook exits 0 silently on every path with a watchdog at the 250 ms wall budget; the spool encodes each record's size in its file name so the 100,000-record and 256 MiB bounds are checked from one directory listing.

## Consequences

- SPEC/MILESTONES describe these as the current design; a later reducer version must migrate the checkpoint representation (SPEC §9.3).
- M2 owns mod-based native turn identity and outcomes, inventory wait mapping and the native Mark handled control; M5 owns OS delivery of canonical outbox intents.
