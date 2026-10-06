# D-0006 — M0C environment limits and retired window shells

**Status:** ACCEPTED by independent review on 2026-10-06 of candidate `6f903c273eedc135123e3a7968d3c00887e8cc70`. These specific dispositions do not accept M0C or close its G08/G09/G15/G16 findings.
**Affects:** SPEC §13.3, §18.5, §21.5; MILESTONES G08/G16, M0C, M1 and M15.

## C-08 — Full Terminal.app restart

**Native status: BLOCKED; not executed on this owner environment.** Quitting Terminal would terminate unrelated owner sessions, including the harness's host. M0B/M0C demonstrate tab close/recreation, TTY reuse and conservative stale-route refusal. The route adapter brackets enumeration with Terminal ProcessKeys and rejects a changed generation; unit tests cover both a stale generation and change during enumeration (`crates/surfaces/src/lib.rs`, `tests.rs`).

Accept the generation/invalidation architecture for M0C while deferring this physical full-application scenario to **M15 clean-user native qualification**. Run the harness outside the Terminal instance being restarted, with ownership of every disposable session in that isolated environment. Record the old/new Terminal ProcessKeys, refusal of old bindings and fresh proof before restored routing. A Terminal generation change always invalidates affected bindings. The separate missing G08 fullscreen and close-race witnesses still block M0C.

## C-09 — Physical external-display disconnect

**Native status: MANUAL_EXTERNAL_REQUIRED; not executed.** The qualified machine has only a built-in Retina display. G16 records live 2× → 1× → 2× DPR, drawing-buffer adaptation, fullscreen, bounds/offscreen correction, keyboard/accessibility and Reduce Motion (`evidence/M0C/g16-window/20261006T140724Z-prod/`). This supports the built-in-display foundation for M1 without certifying external unplug/reconnect.

Assign actual unplug/reconnect to **M15 native hardware qualification**, before that transition is declared supported. Record the hardware, display inventory, live pixels/DPR and safe window restoration. If no external hardware is available, retain the unqualified facet and an explicit built-in-only release profile. No acquisition or connection of hardware is required merely to begin M1.

## C-04 — Retired native window shells

**Nonblocking for M0C, mandatory closure before M1 acceptance.** Ten repeated recoveries left native shells **18 → 28** while retiring the content/cache (`evidence/M0C/view-recovery/20261006T144300Z-prod/summary.json`). The reproduction note reports approximately 33 KB physical footprint per recovery and one WebContent process (`evidence/M0C/attempts/g15-graphics/20261006T142000Z-prod/ATTEMPT-NOTE.md`). This is a finite-run measurement, not a universal bound or independently isolated framework root cause.

Recovery is serialized and delayed, but lifetime recovery/shell count is not capped. Persistent faults could accumulate resources over a long run; deferral to M13 is therefore rejected. **M1 owns a destruction-path repair or requalified dependency update.** After repeated retired-view destruction and quiescence, require `nativeWindowShells.after == nativeWindowShells.before`, no sustained resource growth, bounded WebContent processes and reclaimed caches, while preserving projection, pending intents, bounds/visibility and incarnation rejection.

## Gate semantics

G08/G16 may pass only their explicitly revised M0C scope; the two unexecuted physical facets retain the statuses above. These decisions waive no identity, durability, supervised-admission, wrong-route or pending-resource safety invariant. G17 stays BLOCKED until the independent review's concrete code and native-evidence findings close.
