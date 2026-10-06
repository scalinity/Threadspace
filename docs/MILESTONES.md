# THREADSPACE — Implementation Milestones

**Architecture date:** October 5, 2026, America/New_York  
**Normative architecture:** [SPEC.md](SPEC.md)  
**Status:** Implementation plan. No native qualification, provider E2E or performance gate is represented as already passed.  
**Target:** Daniel's Apple Silicon Mac, macOS 26 or later; Tauri 3, Rust, React/TypeScript, Three.js WebGPU/TSL, independently supervised native companion and SQLite.

## Execution contract

The dependency graph controls progression. **M0A → M0B → M0C → M1–M6 constitute the first usable local MVP.** M7–M15 complete the wider product and release qualification. Optional external integrations must not block local polish or personal release. A capability cannot inherit another adapter's proof. Qualify the minimum native substrate in M0A, reach the decisive real Claude/Terminal breakthrough immediately in M0B, then complete all seventeen retained platform obligations in M0C before M1. Each phase is separately executable and reviewable; extensive office art is not an early prerequisite.

| Path | Dependencies |
| --- | --- |
| Required MVP | M0A → M0B → M0C → M1 → M2 → M3 → M4 → M5 → M6 |
| Local product/release | M6 → M7 → M12 → M13 → M14 → M15 |
| Additional terminals | M8 after M7 |
| Semantic MCP | M9 after M6 and qualified current-session enrollment |
| Remote collector | M10 after M6; its native proof requires a second qualified endpoint |
| ChatGPT experiment | M11 after M6 and the shared URL-surface contract; no remote-machine prerequisite |

M12–M15 include any extras already advertised as supported. Unavailable M8–M11 profiles can remain NOT_SUPPORTED and disabled while the local release proceeds; their own native gate must pass before support is advertised.

SPEC.md owns terminology, contracts and architecture; this plan owns dependencies, workloads and exit evidence. Changes to identity, provider interpretation, native dependencies or durability require a decision record and updates to both documents. Failed prerequisites remain BLOCKED; they do not authorize Tauri 2, Electron, browser-only proof or silent WebGL acceptance.

Each milestone or M0 phase supplies its own `evidence/<milestone>/manifest.json` (for example `evidence/M0B/manifest.json`): application/source commit, exact dependency and lockfile hashes, OS/hardware/build identity, fixture hashes, commands, timestamps, outcomes, known limitations and artifact paths. Preserve failures alongside the final passing run. Native results identify the exact application and companion executable tested. Redact prompt bodies, tool payloads, credentials and personal filesystem details from shareable evidence.

Gate verdicts are PASS, FAIL, BLOCKED and NOT_SUPPORTED; NOT_RUN is a progress marker only. An optional external profile can remain explicitly unsupported without blocking an unrelated milestone; list its disabled behavior and reason. This convention cannot waive a required MVP capability or one of the seventeen M0 checks. An implementation screenshot, source inspection or synthetic replay never substitutes for native provider, notification, routing or graphics evidence.

Establish project-owned runners for qualification, deterministic replay, native tests and benchmarks. Their manifests distinguish Rust/domain tests, Tauri 3 mock IPC, browser/DOM tests, real macOS qualification builds and the uninstrumented installed package. Native automation should use a Tauri 3-compatible harness and XCTest/XCUITest where useful. The researched WDIO native packages depend on Tauri 2 and are not the selected harness. [WDIO manifests][wdio] [Tauri 3 tests][tauri-tests]

Every exit review checks zero known wrong-session routes, duplicate canonical workers, false native terminal outcomes, lost durably accepted owner actions or observer-caused provider decisions. Performance targets are measurements to obtain, not claims made by this plan.

## Platform qualification — M0A → M0B → M0C

**Purpose.** Retire architecture-breaking external assumptions through three separately executable, reviewable phases. Reach the real Claude/Terminal correlation breakthrough before extended stress, recovery or window polish. Each phase has its own commit and `evidence/M0A`, `evidence/M0B` or `evidence/M0C` manifest; no phase claims native proof from source research.

**Dependency rule.** M0A establishes the minimum native substrate; M0B follows immediately and is the decisive identity/Return GO/NO-GO. M0C closes the remaining platform and provider qualification. M1 requires all three. Independent source/fixture work may proceed in parallel, but the M0B native proof must not wait for M0C sleep, stress, full mod semantics, Codex or visual polish. A failed identity edge is explained and repaired before substantial dependent product construction.

**Prototype contract.** Start from an empty repository with only the reusable modules needed by these proofs: exact build configuration, native outer bootstrap and one companion app, private relay and single-writer SQLite fixture, minimal Session/ProcessKey/activation/SurfaceBinding/Turn/AttentionItem records, the actual five UI bridge commands, one Terminal worker and a tiny TSL scene. A bounded fixture reducer is sufficient here. Full entity coverage, retention, generalized reconciliation, actor graphs and final attention policy belong to M1 onward. Reuse the proven adapters and substrate; do not build a throwaway second architecture. Qualification-only unimplemented operations return typed errors and are never advertised as product features.

**Implementation requirements.** Freeze the coherent candidate below and commit the application's own lockfiles. The Tauri source reference is `a8703ee487c659efbebb27c799752d523a6d09a1`; its release is an alpha despite inconsistent GitHub prerelease metadata. [Release][tauri-release] [Manifest][tauri-manifest]

| Dependency | Exact qualification candidate |
| --- | --- |
| Rust toolchain / target | `1.99.0`, `aarch64-apple-darwin`; edition 2024 |
| `tauri`, `tauri-runtime-wry`, CLI | `3.0.0-alpha.4` |
| `tauri-build`, `tauri-runtime`, `tauri-utils`, `tauri-macros`, `tauri-codegen` | `3.0.0-alpha.3` |
| `tauri-plugin` infrastructure, if used | `3.0.0-alpha.3` |
| `@tauri-apps/api` | `3.0.0-alpha.2` |
| CLI's resolved bundler / macOS signing crate | `3.0.0-alpha.3` / `3.0.0-alpha.2` |
| Resolved Wry / Tao | `0.57.0` / `0.37.0` |
| Three.js | `0.186.1`, r186 candidate source `9b4a2ac29c63ccb43fd51c5661f2f873ac2c39b8` |
| SQLite | `3.53.4`, linked into the native companion; verify `sqlite_version()` and `sqlite_source_id()` |
| Claude / Codex initial profiles | Claude `2.1.290`; Codex `0.160.1`; qualify installed CLI and desktop runtimes separately |

Version sources include the [Rust release][rust], [Three package][three-package], [SQLite release][sqlite-release], [Claude release][claude-release] and [Codex release][codex-release]. Generate declarations from the installed Claude build; the older public declaration snapshot is not automatic certification of the newer CLI.

Construct the explicit runtime with `.runtime(tauri_runtime_wry::Wry::default())`. Use the selected v3 extension traits, `DynRuntime` defaults and main-thread APIs. Remove obsolete `macos-private-api`/`app.macOSPrivateApi` settings. Record exact frontend dependencies, macOS build, SDK, WebView version and asset hashes. Commit `Cargo.lock`, `package-lock.json`, `rust-toolchain.toml` and `docs/compatibility/platform-lock.json`. A newer prerelease requires a new compatibility run, not a floating dependency update.

Keep provider/process/TTY/AppleEvents, UserNotifications, SQLite and domain settings in direct native Rust/Apple bridges; the outer native bootstrap owns ServiceManagement calls. No optional Tauri plugin is selected. Official v3 plugins exist, but manifest compatibility is not native compilation proof. No Tauri 2 plugin/runtime/test family enters the dependency graph. Generate the nonempty application command manifest and the one `office-local` webview capability defined in SPEC Section 18.6; test effective permissions, not only configuration text. [V3 plugin manifests][plugins]

### Traceability of the original seventeen M0 gates

Every original G01–G17 obligation and numerical workload remains below. This table maps **all** portions to their phase. A gate with several portions is PASS only when each mapped portion passes; an M0A smoke is not the final G02/G03/G04/G06/G09/G10/G16 sign-off. Unchanged passing evidence can be reused; changes to its implementation, dependency, signing identity or native runtime require affected regression.

| Original gate | M0A foundation | M0B decisive spike | M0C closure |
| --- | --- | --- | --- |
| G01 minimal build | Complete pinned arm64 build and bridge/ACL compile | Reuse | Regress if changed |
| G02 dev + package launches | One launch in each context; package independent of checkout | Reuse installed candidate | Complete original ten launches of each context |
| G03 request/response IPC | Typed success/error plus actual app-manifest/ACL smoke | Real Return command | Complete 1,000 round trips and cancellation/close/malformed/origin/view tests |
| G04 sustained Channel | Real bounded snapshot, patch and ACK | Stream real identity/binding changes | Complete 10,000 changes/60 seconds; bounds, paging, ACK stalls, gaps, cache cleanup and recovery |
| G05 notifications | Complete actual helper permission/settings/granted/denied and UI-quit send | Reuse | Recheck if lifecycle/signing changes |
| G06 notification interaction | UI-open and UI-quit basic callback/cold intent | Connect available Return path | Complete ten interactions, resolved/stale targets and recovery |
| G07 native mechanisms | Bounded argv, Unix peer checks, basic native bridges/preferences | Real Claude ancestry, birth/executable and controlling-device proof | Codex detached-hook ancestry and remaining mechanism cases |
| G08 Terminal inspection/return | Dictionary and bridge availability | Three same-cwd sessions; ten routes each; reorder/move/close/recreated-tab rejection | Remaining minimize/restart/Spaces/readback-race negatives |
| G09 independent observer | Single supervised app; UI quit and Tauri dev-restart independence | Reuse background substrate | Helper kill/crash restart with no new hook; continued durable capture |
| G10 SQLite persistence | One writer, linked engine, WAL/FULL commit/ACK and reopen | Persist actual correlation records | Before/after commit/ACK crashes and checkpoint fixture replay |
| G11 restoration | — | Basic observed Session continuity | All ten UI/helper restarts; native same-label view incarnation |
| G12 sleep/wake | — | — | All five actual cycles |
| G13 actual renderer init | Complete tiny bundled TSL scene in dev and package | No art dependency | Regression after lifecycle work |
| G14 actual WebGPU | Complete backend attestation plus forced-WebGL2 negative | Reuse | Recheck final accepted package |
| G15 packaged animation | Brief changing scene only | No sustained/polish prerequisite | Full fifteen minutes, actual pixels, hidden/rebuild/loss/resource tests |
| G16 native window/titlebar | Traffic lights, drag regions and minimize smoke | Real focus interaction | Full fullscreen/display/Retina/restoration/keyboard/VoiceOver matrix |
| G17 final platform disposition | Record open native risks | Explicit correlation GO/NO-GO contributes | Final sign-off across all gate portions and provider profiles |

G09's lifetime requirement is unchanged. Its former custom `SMAppService.agent`/`RunAtLoad`/`KeepAlive` mechanism is replaced by Apple's documented single login-item app relaunch contract, preserving every UI-close/crash/dev-restart/helper-restart/durable-capture exercise while removing the custom LaunchAgent job. A normal successful helper exit is not assumed to restart; the enabled companion must remain in its AppKit run loop, and unexpected fatal exits are nonzero. [Apple launch/relaunch][service-register]

### Retained gate obligations and passing evidence

| Gate | Required check | Passing evidence and exercise |
| --- | --- | --- |
| G01 | A minimal Tauri 3 application builds. | Clean arm64 build; exact versions and `cargo tree` captured; explicit Wry runtime; no Tauri 2 or unintended CEF dependency. Compile the actual native bridge and command permission configuration. |
| G02 | Development AND packaged applications launch. | Ten launches of each context. Packaged run loads assets without Vite or the source checkout. Enforce the macOS 26 floor at runtime as well as bundle configuration. |
| G03 | Rust↔TypeScript request/response IPC works. | Typed success/error, cancellation, closed-window and malformed input tests; at least 1,000 round trips. Reject unauthorized origins/webviews and disallowed commands in the actual v3 configuration. |
| G04 | Rust→frontend streaming/events survive sustained load. | At least 10,000 committed synthetic changes over 60 seconds through native Channel; exact final projection, bounded buffers, old-epoch rejection, delayed ACK and independent reconnect. Apply the snapshot/window contract below. |
| G05 | Native notifications work. | System-started login-item companion submits notifications with the main UI quit; verify real permission/settings state, granted/denied paths and stable attention identifiers. |
| G06 | Notification interaction routes back into the application. | Ten notification interactions across UI-open, UI-quit and already-resolved states; native delegate reloads the item and opens the correct inspector/return flow. Cold-start intent waits for hydration. |
| G07 | Rust executes the required local macOS mechanisms. | Bounded argv execution, native process birth/ancestry/TTY sampling, Unix peer checks, AppleEvents and native preferences. Capture detached-hook ancestry for both providers; never infer source TTY from hook stdin. |
| G08 | Terminal.app can be inspected and routed by the selected native/AppleScript strategy. | Three independently opened tabs, including same-cwd sessions; ten routes per target plus tab reorder/move, minimize, close and restart negatives. Verify selected TTY, frontmost app and fresh provider/process identity; no injected input. |
| G09 | Hook/event endpoints stay alive independently of the 3D renderer. | The selected single `SMAppService.loginItem` launches the signed nested AppKit companion and provides documented crash/nonzero relaunch; no parallel LaunchAgent registration. Close UI, kill UI, stop Tauri dev and crash helper; service restarts without a new provider hook. Confirm continued durable capture. |
| G10 | SQLite local persistence works. | Single writer, WAL/FULL, committed receipt semantics, reopen and checkpoint replay. Kill before/after commit and ACK; every acknowledged record survives ordinary process crash, duplicates remain idempotent. |
| G11 | Restart and state restoration work. | Ten UI/helper restart cycles preserve a small fixture's identities, turns, owner commands and unresolved attention. Same-label recreated WebView gets a new epoch. |
| G12 | Sleep/wake does not corrupt state. | Five actual sleep/wake cycles while capture/IPC is active; revalidate native sources and UI transport; no invented completion, lost accepted state or stale-route success. |
| G13 | Three.js WebGPURenderer initializes inside the actual Tauri 3 macOS WebView. | Async initialization in both contexts; bundled TSL scene, hit testing and assets work inside Wry/WKWebView, not only Safari/Chrome. |
| G14 | The application reports the actual WebGPU backend on the target. | Record origin, secure context, GPU availability, initialized backend, device and Three revision. A forced WebGL2 run is visibly diagnostic and fails this primary-backend check. |
| G15 | A sustained animated Three.js scene is stable in the packaged application. | Fifteen-minute animation with changing native state; presented screenshots agree with DOM/AX and journal revisions. Exercise hidden/resume, device-loss recovery and resource requests during close/reload. |
| G16 | Native-feeling window/titlebar behavior uses current supported APIs. | Traffic lights, fullscreen, drag regions, focus, Retina/DPR change, external monitor disconnect, restored bounds and keyboard/VoiceOver operation; no deprecated/private enabling workaround. |
| G17 | No unresolved Tauri 3 alpha issue invalidates the core architecture. | Review every preceding result plus provider probes, native ACL, resource cancellation and bundle supervision. Resolve/contain every reproduced architecture-breaking defect with a pinned fix and regression. Unknown required behavior cannot be signed off. |

## M0A — Tauri 3 / Native Platform Foundation

**Purpose.** Prove that the native shell and independent observer substrate actually work.

**Prerequisites.** Target Apple Silicon Mac/macOS 26; installed Terminal.app and its dictionary; selected stable local signing identity. Use separate development bundle IDs, service identity, stores and sockets. Real Claude/Codex prompts and complete mod qualification are not prerequisites for this phase.

**Build order.** Compile the pinned outer app and native bridges; construct/sign the single `ThreadspaceAgent.app` under `Contents/Library/LoginItems`; register it from the stable outer development host; prove private IPC and a committed/reopened fixture; connect the five Tauri commands and one real Channel snapshot/patch/ACK; exercise the actual companion's notification send/click path; initialize the tiny bundled TSL scene and attest WebGPU in dev and packaged execution. Keep native window work to the G16 foundation subset.

The native outer bootstrap owns service registration/status/unregistration, including when no WebView exists. The companion owns the only journal writer, notifications and Terminal automation. Pre-sign its complete bundle/entitlements, copy it, then sign the outer app and verify both. A raw Tauri-spawned sidecar does not qualify. The package must work with Vite and the checkout unavailable. [Bundler][tauri-bundler] [Login-item application][service-login]

For transport smoke, install the Channel handler before connect. Use the selected complete snapshot cap of 512 KiB/ten frames, 64 KiB per frame, and 32-frame/2 MiB unacknowledged window. Apply SnapshotEnd atomically before ACK. Initial foundation needs one real round trip and state change; M0C performs the full workload and fault suite.

Request/query actual notification authorization from the system-started helper, strongly retain its delegate before AppKit finishes launching, send with the UI quit, and open the correct hydrated fixture inspector from a real interaction. A successful permission call without an actual banner and callback is insufficient. A denial run must leave durable fixture attention accessible. This application-context proof is mandatory, not deferred behind the graphics work.

**Acceptance.** Every M0A cell in the traceability table passes. The companion survives UI quit and a Tauri dev restart. Linked SQLite reports 3.53.4/source ID and commits/reopens one fixture through its single writer. The actual initialized renderer reports WebGPU in both native contexts; a forced compatibility run is labeled and cannot satisfy G14. Basic titlebar/window controls work. Native notification send/click and cold intent are real, with exact bundle identity recorded.

**Non-goals.** Full reducers, provider graph/provenance semantics, complete recovery, long animation runs, release notarization or final office art.

**Evidence and exit.** Exact locks/build/signature tree, native command/Channel fixture, journal receipt/reopen evidence, notification/settings/callback recording and backend diagnostics. M0A PASS authorizes M0B immediately. Any required substrate failure remains BLOCKED; native probes cannot be replaced by browser mocks.

## M0B — Decisive Claude Identity + Exact Return Spike

**Priority.** HIGH. This is the architecture's decisive GO/NO-GO proof.

**Purpose.** An ordinary Claude conversation manually launched outside Threadspace is found as the correct persistent worker and returned to through its actual process, controlling TTY and current Terminal tab, including concurrent sessions sharing the same cwd.

**Prerequisites.** M0A; installed Claude candidate from the platform lock; explicit reversible test integration setup. M0B itself qualifies that candidate's identity/current-session-routing subset. Full mod accepted-input semantics, actor graph, Codex, final attention policy and M0C reliability workloads do not block this spike. Reuse M0A's real companion, journal/IPC fixture and compact UI; a small identity table and Return buttons suffice.

**Implementation.** Implement SPEC Sections 4.4–4.6 and 13.3 in their assigned native adapters. The first inventory response enumerates candidates; incumbent kernel samples bracket a further bounded native lookup before admitting session→process proof. Require full ID, qualified direct interactive mode, ProcessKey, executable, valid controlling-device evidence and a unique current Terminal `tty` path whose character-device `st_rdev` agrees. Validate every captured ancestry edge. Retain conditional/missing fields and unsupported/background clients honestly. No launcher, nonce, cwd, terminal title, recency or frontmost-window guess may supply the identity join.

**Required real sequence.**

1. Open Terminal.app manually outside Threadspace; record its application incarnation and installed dictionary.
2. Launch a normal direct foreground Claude conversation manually. Respect existing launch preferences; an agent-view/background client is a different profile, not evidence of a direct conversation.
3. Start Threadspace after that Claude process is already running and recover its full persistent native identity through supported inventory. Prove this lookup without relying on a newly emitted SessionStart hook or a preexisting Threadspace binding.
4. Obtain the real provider PID and bracket its kernel birth/executable/boot evidence as specified; persist the accepted ProcessKey separately from the Session.
5. Obtain the provider's controlling-device evidence; do not read hook stdin or the detached hook's `/dev/tty` for it. Capture actual hook ancestry separately and document when inventory supplies the usable path.
6. Enumerate current Terminal windows/tabs and find exactly one matching character device. Verify native selected-tab/frontmost readback through the packaged companion's actual authorization identity.
7. Demonstrate Return-to-Agent, including fresh provider-currentness and process/binding validation around focus. Record all three result axes; a successful AppleScript exit code alone is insufficient.
8. Manually start two more distinct Claude sessions in the same cwd. Prove three distinct persistent sessions/process relations and independently correct bindings.
9. Execute ten verified routes to each of the three targets. Completing or selecting one cannot alter another's identity or route.
10. Complete a real turn and show that the same live Session remains present. Record actual provider/operator evidence; do not turn Stop or inventory idle into an invented native terminal outcome merely to finish the spike.
11. Submit another real prompt and prove that its persistent Session identity remains unchanged. Full automatic attention resolution is qualified in M0C/M2, not inferred here.
12. End/detach the old activation as the provider supports, then resume that same native Session in another independently opened tab. Preserve one worker and create fresh activation/surface proof. Retire old route authority only on proven end/supersession.
13. Attempt a simultaneous resume/attachment. Record actual acceptance or provider refusal. Refusal must create no phantom attachment or displaced route; supported coexisting attachments must require a chooser. Multiple-attachment state permutations remain required synthetic coverage even if native concurrent writers are refused.
14. Reorder/move tabs and close/recreate a target. Prove fresh enumeration still selects the right live target and stale ProcessKey/TTY/binding generations cannot revive an old route. Native identity changes and synthetic PID/TTY reuse exercise the same adapter rejection logic.
15. Exercise supported in-place clear/resume and, where the runtime allows it, A→B→A. Prove the inventory actually reports the current direct conversation; use synthetic A→B→A for unavailable native transitions and record that limit. A provider's stale association or missing ID never becomes current merely by waiting or rereading it.
16. Demonstrate an ambiguous/unsupported case, such as an unbound background client or multiple eligible bindings. Return a typed ambiguity/inspector result without newest/frontmost/same-cwd selection or unintended focus.

**Evidence.** Uncut real-Claude/Terminal recording; full-ID→ProcessKey→device→tab proof ledger; inventory request intervals and before/after kernel samples; runtime/inventory version provenance; thirty successful route readbacks; resume and stale-binding negatives; explicit capability and failure-edge disposition. No native PID reuse or GPU fault is claimed from a synthetic injection.

**Exit — GO/NO-GO.** GO requires the actual manual direct-Claude path, late startup, three same-cwd sessions and freshly verified exact Return to pass. A missing current-session lookup, unsafe incarnation join or unreliable native readback is NO-GO and leaves M0B BLOCKED pending a concrete supported repair. Lower-proof bindings remain useful product data but cannot close this primary gate. M0C begins after GO; substantial downstream implementation cannot hide an unresolved identity edge.

## M0C — Platform Reliability / Recovery Qualification

**Purpose.** Prove that the successful substrate and real identity path survive restarts, loss, sleep and native application behavior.

**Prerequisites.** M0B GO. Use the same source/lock family and actual native app identities; record changed artifacts and repeat affected prior proofs. Codex test setup and exact installed Claude mod declarations enter here.

**Scope and acceptance.** Complete every M0C cell and every remaining original numerical workload in the G01–G17 ledger. This includes ten dev/package launches each; 1,000 IPC round trips; 10,000 committed streaming changes over 60 seconds; ten notification interactions; ten UI/helper restart cycles; five actual sleep/wake cycles; fifteen minutes of changing packaged animation; and all listed native surface/window/resource negatives. G17 reviews all accumulated portions, not just this phase's tests.

**IPC and view recovery.** Serialize AttachView at cursor S before subsequent changes. Test oversized snapshot/paged data, unloaded entity patches, delayed/impossible ACKs and independent status recovery. Enforce the five-second hydration deadline, two-second visible heartbeat and independent invoke status after five seconds without progress. A missing Channel frame cannot trap its own reset. Account for stream and ordinary callback-response caches; retire and destroy/recreate the actual office view when sent data may be unconsumed, wait for native removal before label reuse, and preserve bounds/visibility/pending intents. Page reload or Channel drop alone is insufficient. [Channel source][tauri-channel]

Validate an app-owned native incarnation marker from each incoming WebView's public resource table, including bootstrap/connect; label equality cannot certify the same view. Carry native/view/subscription/core/store context through queued work. A retired view cannot start unaccepted mutations or focus; a committed owner command stays retrievable by request ID. Capture native construction markers in page/close closures and ignore retired callbacks. Recreate the actual office view for main-document replacement after its initial load. Test a first-ever delayed old connect across document replacement, late old-view page callback after label reuse, old action/query and same-label recreation, bootstrap with the companion unavailable, second UI launch with observation disabled, and stale/future ACKs. Keep data-query concurrency/reply limits from SPEC Section 18.3.

**Companion, storage and notifications.** Kill the UI and helper separately. With the UI quit and no new hook, kill/crash the helper and prove independent system restart, one writer and resumed capture. Exercise process/ACK crash boundaries, checkpoint fixture restoration, settings denial/revocation and actual notification callbacks during UI quit/helper recovery/disabled observation. Test prepare-before-unregister and a preparation failure: no drain/backup is assumed possible after unregistration. The login-item app stays alive while enabled; crash-relaunch and successful deliberate-stop behavior are distinct. Bundle signing and actual AppleEvents/notification sender identities must pass on the installed artifact. [Service semantics][service-register] [Termination on unregister][service-unregister]

**Provider semantics.** Record safe hook installation and detached-hook ancestry for both providers. Generate/pin the installed Claude declarations; qualify one-call unchanged middleware/generator forwarding, native outcomes, original human-input/core-acceptance proof, actor spawn/list provenance, reload and logical-session changes. Test plugin-raised lifecycle events and downstream middleware short-circuiting core. Qualify the mod-batch receipt and partial-commit retry path. Do not install WorktreeCreate/WorktreeRemove handlers. Record unavailable capabilities and classic-limited behavior. Qualify accepted-input provenance separately from a positive original-order witness for automaticHumanFollowupResolution. If that witness is absent, record this facet NOT_SUPPORTED/disabled and verify explicit Mark handled; native outcomes and exact current-session Return remain required. This evidence-based mod limitation does not waive any G01–G17 platform obligation. Probe Codex modes and the existing daemon's actual protocol/version without starting/resuming provider work. Full M6 integration remains later. [Claude hooks][claude-hooks] [Mod reference][claude-mods] [Codex schema][codex-schema]

**Graphics and window reliability.** Inspect actual changing packaged pixels, not only live DOM/AX. For this Three pin, nulling the render callback leaves internal RAF work: dispose on confirmed hide/minimize/2D, retain CPU state, and rebuild a fresh attested renderer on visibility. Count internal scheduled work, test hide during init and repeated transitions, and serialize generations. Inject a qualified loss callback against a real renderer to test recovery when natural loss is unavailable, labeling it injected; `GPUDevice.destroy()` alone does not trigger the pin's normal loss handler. Finish fullscreen/Spaces, minimize/restore, Retina/DPR, display disconnect, native bounds, keyboard/VoiceOver and resource-cancellation tests.

**Non-goals.** Complete M1 domain logic, final office art, every terminal, consumer ChatGPT or public notarization.

**Evidence and final platform exit.** Consolidated seventeen-row checklist with each A/B/C evidence reference; exact versions/declarations/dictionaries; native failure reproducers, signature tree, notification/route/readback recordings, actual backend/pixel and resource measurements. All required portions PASS and no unresolved alpha/native defect invalidates the architecture. M0C PASS authorizes M1. The source-supported design is never described as natively passed before these artifacts exist.

## M1 — Journal, contracts and deterministic synthetic harness

**Purpose.** Establish the durable, replayable truth that every subsequent vertical slice uses.

**Prerequisites.** M0C, including M0A substrate, M0B identity/Return GO and the complete retained platform ledger.

**Scope.** Promote the reusable M0 substrate into the SPEC ownership layout, creating only modules now needed; complete Rust contracts and generated TypeScript/JSON Schemas; canonical reducer; SQLite journal/projections; private relay/spool; initial attention and notification-outbox state; synthetic provider and CLI replay runner.

**Implementation requirements.** Implement distinct Session, Actor, ExecutionInstance, ProcessIncarnation, Turn, InputAttempt, SourceSurface, SurfaceBinding and AttentionItem records. Enforce namespace/native-session uniqueness. Source epochs, callback sequence, ingest cursor, activation and native turn identity remain separate. Normalize to native-keyed drafts, resolve/allocate and record canonical IDs inside the single-writer admission transaction, then reduce resolved facts without random/time reads. Journal facts and owner commands; atomically commit projection, attention, outbox and cursor. Causal comparison is scoped and can be incomparable; retain accepted-human frontiers and wait-clear barriers for reverse arrival and compaction. OS side effects execute after commit.

Implement fail-open bounded capture: same observation UUID through retries, commit-only ACK, atomic ready spool, quarantine and saturation diagnostics. Apply SPEC's input/depth/frame limits, private socket ownership and single-writer lock. Unknown discriminators cannot drive state. Default sanitization drops prompt/tool bodies and transcript content.

Synthetic scenarios cover every canonical transition and all requested failures: parallel tools, children, waits, completion/follow-up, interruption/refusal/failure, resume/end, duplicates, causal permutations, stale snapshots, process reuse, dropped observations, restart and routing ambiguity. Use deterministic seeds, a virtual clock and evidence-bearing native-shaped fixtures without model calls.

**Non-goals.** Expanding provider integrations or polished scene animation. Keep M0B's proven real identity/Return slice runnable while completing the canonical engine; minimal presentation records verify reconstruction. Do not replace it with a backend-only restart.

**Tests.** Pure reducer and schema tests; SQLite crash/receipt tests; fixture replay; malformed/sensitive input cases; property tests over at least 10,000 seeded valid partial-order permutations. Replay completion/input-entry/input-acceptance/owner-command permutations, including reverse delivery, upstream-delayed submission and queued-before-output cases. Include ingest-cursor gaps without false event-loss alarms, generation-scoped wait clear before delayed positive, and committed command retry after its original revision advanced.

**Quantified acceptance.** Exact hashes for replay of the same admitted journal/checkpoint; native-key-normalized semantic equality across independently admitted permutations, excluding random IDs/cursors/receipt times/allocation-order layout; zero duplicate sessions/facts after ten deliveries of each observation; PID/TTY reuse never revives a binding; terminal turn outcomes never regress from late tool activity; Stop never ends an execution. One hundred commit/ACK crash injections preserve every durably acknowledged record. Normal capture p95 ≤25 ms and application wall budget ≤250 ms on the native fixture; failure paths exit 0 without provider-control output.

**Required evidence/artifacts.** Versioned schemas/migrations; generated types; fixtures and seeds; reducer invariant catalog; journal/receipt crash matrix; sanitized-record snapshots; replay hashes; saturation and capture timings.

**Exit gate.** All canonical states and owner commands reconstruct deterministically, ingress remains bounded/fail-open, and no known invariant failure remains.

## M2 — Manually launched Claude vertical slice

**Purpose.** Make the primary workflow useful end to end using an independently launched real session.

**Prerequisites.** M1, M0B's proven direct-Claude Return path and the exact M0C-qualified Claude native-observer profile.

**Scope.** Explicit reversible setup; command registration; native inventory; observer mod; one Terminal surface adapter; minimal project desk/worker; fleet/inspector/attention controls; native Return-to-Agent.

**Implementation requirements.** Preserve existing hooks/settings with backup, atomic writes and ownership markers. Install only passive handlers. The observer calls `next(e)` once with unchanged event/result and forwards generator chunks correctly; capture failures never change provider behavior. Freeze callback-entry session/actor/turn/activation context and host-stamped dispatch origin before awaiting results. Plugin-raised lifecycle-shaped events cannot establish native outcome/termination solely from their payload. Require qualified engine provenance/semantics or independent native corroboration. Inventory waits remain session-scoped when no turn/actor identity exists; a completed parent and waiting child must not reopen the parent Turn. Drain sanitized mod observations through documented host `$.process.run` with argv, JSON stdin and `timeoutMs`; use the separate typed `mod-batch` receipt, retaining unknown/unaccepted records with stable UUIDs. Conventional hooks remain silent/exit0. Keep one bounded in-flight drain and SPEC's queue/batch limits. Engine-origin spawn requests still need actual core spawn proof; arbitrary middleware host-list/session results are not independent corroboration.

Reuse M0B's full native `session_id`→bracketed ProcessKey/executable→controlling character-device→unique Terminal tab proof. First inventory rows are candidates; current lookup must occur between incumbent kernel samples. Background-worker PIDs do not identify attached terminal clients. Use `claude agents --json --all` for fresh inventory and current-session route verification; retain snapshot interval/epoch. Do not require a wrapper, Threadspace-created terminal or launch nonce. Native acceptance/provenance and a separately qualified positive original-order witness control automatic follow-up resolution. Without the latter, preserve the output and provide explicit Mark handled; keep the native observer's other proven capabilities. [Native inventory][claude-inventory]

Render one simple worker from canonical view state and provide an explicit Return button. Click selection alone opens the inspector. Route by fixed bundled AppleScript with data argv, readback and generation checks; default fallback NONE. Surface result, session verification and input readiness are separately visible.

**Non-goals.** Final art, teams, full worktree layout, iTerm/Ghostty/tmux, Codex or a general launcher. Native notification production is M5; M0A/M0C already proved the platform mechanism.

**Tests.** Open Terminal outside Threadspace; enter `claude` manually; submit a real tool-using prompt; complete; return; submit follow-up; exit. Separately exercise Stop continuation, interrupted/failed native turn, blocked submission, relay absence and an incompatible mod profile. Raise a plugin turn.complete with a real turn ID and confirm no native terminal outcome or automatic owner-completion item. Test parent-completed/child-waiting inventory and retain aggregate session attention without changing sibling Turns. Host fixtures verify downstream middleware returning without core acceptance, forged spawn results, provider exceptions and generator cancellation do not create false acceptance/actors or call `next` twice. Test upstream-delayed human submission and preserve its native active-at-submission turn identity. Test mixed mod-batch receipts, partial commit then timeout, exit0 with failed spool, malformed receipt and unchanged-ID replay.

**Quantified acceptance.** Ten repeated prompt/completion/follow-up cycles keep one Session and worker; completion never removes it. Thirty successful exact-current-session routes reach the right live tab; all negative routes return an honest typed failure without moving to an unrelated target. Existing hooks retain behavior; ten install/reinstall/remove cycles create no duplicate handlers and preserve unrelated settings. Capture/commit-to-view meet the normal latency targets.

**Required evidence/artifacts.** Uncut native vertical-slice recording; redacted hook/mod/inventory trace joined to journal IDs; install diff/rollback; tab readbacks; native outcome and acceptance fixtures; route timings.

**Exit gate.** A normal manually launched Claude session is discovered, works, completes while staying present, accepts follow-up as the same worker, and returns to its freshly verified original surface.

## M3 — Identity, reconciliation, same-cwd and resume

**Purpose.** Prove that the useful slice remains truthful across ambiguity, concurrency and observation gaps.

**Prerequisites.** M2. Retain its native provider and terminal profiles.

**Scope.** Full activation/source generations, process monitoring, multiple attachments, late discovery, snapshot/history reconciliation, crash recovery and route invalidation.

**Implementation requirements.** Preserve one canonical Session per namespace/native ID and one presentation identity across resume. New native IDs create new Sessions even at the same cwd. Model A→B→A activation changes independently of mod reloads. An old SessionEnd closes only its matched activation. Revalidate `exec` changes as well as PID birth and every captured ancestry edge. A native concurrent-resume refusal creates no attachment/worker/route change; prove allowed coexisting attachments when available and retain synthetic multiple-attachment coverage otherwise.

On startup/gap/wake, restore journal state, subscribe/buffer live observations, obtain bounded inventories and merge through the reducer. Absence from an unversioned list cannot establish deletion, successful completion or resolved requests. Requery conflicts; unknown history stays unknown. Background worker exit does not terminate a provider-retained logical job. Concurrent resumes produce one worker with explicit attachments, never newest-surface selection.

**Non-goals.** Cwd/title/recency heuristics for automatic binding, transcript scraping, automatic provider resume or claiming all lost records are recoverable.

**Tests.** Three same-repository sessions; five same-cwd sessions in a fifteen-session fleet; resume in a different terminal; new-session clear/fork; simultaneous attachments; A→B→A with delayed callbacks; mod reload; provider `exec`; PID/TTY reuse; tab move/reorder/close; Threadspace late installation; sleep; helper outage and spool replay. Include missing-event repair both with and without sufficient native evidence.

**Quantified acceptance.** Thirty routes across the three-session case focus only their requested tab; completing one leaves all unrelated identities/turns unchanged. Twenty resume/new-session cycles create no accidental worker duplicates. Ten force-quit/restart runs with active external sessions preserve accepted state and owner commands. Ten thousand synthetic generation/callback permutations reject stale ownership. Every intentionally unrepairable gap remains visibly unproven rather than silently current.

Force-quit the main UI while the helper captures new work, then separately crash the helper and recover its spool. These prove different failure domains. With the native source unavailable, present recovered attention promptly and label last-observed state stale.

**Required evidence/artifacts.** Session/activation/binding lineage tables; native before/after inventories; restart and late-launch recordings; process-reuse fixtures; reconciliation coverage ledger; same-cwd route matrix; checkpoint/replay hashes.

**Exit gate.** No identity collision, wrong automatic binding, stale activation closure or reconstructed false outcome remains in the specified scenarios. Unknown evidence is visible and actionable.

## M4 — Subagents, teammates and parent-owned work

**Purpose.** Represent concurrent actor trees without confusing child activity with principal sessions or owner requests.

**Prerequisites.** M3 and qualified native actor/spawn/list evidence.

**Scope.** Stable subordinate Actor identities, repeated runs, immediate-parent/tree-root/teammate/fork relations, unresolved relationships, child visibility and opt-in qualified Claude team profiles.

**Implementation requirements.** Upsert native actor IDs in the correct provider scope; repeated subordinate starts can mean another run. Merge explicit spawn/list evidence, never infer immediate parent from the tree root or actor type. Keep current role separate from historical lineage and freeze owner-facing eligibility to the originating run/request. A delayed child completion after root promotion cannot become unsolicited owner attention. An actually provider-supported owner-addressable role change retains persistent presentation identity; selecting a child or an unsupported resume does not promote it.

Parent-owned subordinate completion/failure updates its parent/task view. It does not automatically generate owner attention. Create owner attention only for a proven owner wait, explicit handoff/escalation or independently owner-facing actor. Parent completion neither stops live children nor erases existing owner actions. Collapse internal helper agents by default. Teams remain an explicitly enabled provider profile; do not enable provider experiments during setup. [Claude teams][claude-teams]

**Non-goals.** Agent orchestration, team creation, messaging agents, automatic retry or claiming Desktop supports an unqualified CLI team mode.

**Tests.** One parent with five concurrent children; nested children; duplicate start/stop; child outliving parent turn; repeated teammate runs; vetoed child Stop; orphan then parent discovery; child-as-root resume; parent-owned failure versus explicit owner escalation. Exercise two native teammates when advertising that team profile.

**Quantified acceptance.** Native five-child case and at least 1,000 seeded graph permutations produce stable identities and correct relationships; zero orphan promoted to independent root without evidence; zero routine child-output notification storm. One child failure changes neither sibling outcome nor parent execution presence.

**Required evidence/artifacts.** Native actor fixtures/recording; graph lineage snapshots; repeated-run and role-change assertions; team compatibility profile; owner-attention ownership matrix.

**Exit gate.** Supported actor graphs and run lifetimes are truthful; ordinary children remain parent-owned; every advertised team mode passes its separate native test.

## M5 — Durable attention and native notifications

**Purpose.** Make Threadspace a dependable owner-action inbox while the office is unfocused or closed.

**Prerequisites.** M4; the completed M0A/M0C notification identity/click and independent-service gates.

**Scope.** Complete attention categories, priority/age/pinning, acknowledgement/resolution/snooze, outbox recovery, native delegate, drawer, keyboard actions and notification routing.

**Implementation requirements.** Keep individual turn-output, exact-request, aggregate wait-category and owner-decision identities. OUTPUT_READY and terminal completion update one output item. Native waits require supported current evidence; permission preflight alone does not establish an open human dialog. Semantic owner actions remain separately attributed.

Journal each owner command with an idempotent command ID. Auto-acknowledge only exact native focus plus CURRENT_NATIVE_REVALIDATED and FOREGROUND_COMPATIBLE readiness; backgrounded/unknown jobs require explicit acknowledgement. Enable automaticHumanFollowupResolution only with a qualified positive original-order witness, independently of acceptedInputProvenance. Then auto-resolve only eligible causally prior output after verified accepted human-origin input; otherwise preserve explicit Mark handled. Keep original submission and acceptance points. Scheduled/peer/SDK/unclassified or blocked input cannot clear it. Explicit owner controls remain available in limited profiles.

The single AppKit login-item companion owns UserNotifications and current settings, with its delegate retained before launch completes. Notification-driven starts respect observation-disabled/maintenance state and the writer lock. Stable notification request IDs and a durable outbox survive UI absence. Crash after OS submission creates recorded uncertainty when delivery cannot be settled; do not promise exactly-once visible banners. Re-read attention/binding on every click, including old notification responses. Coalesce banners without merging away queue items. Catch-up replays do no OS delivery: hold historical output intents for eligibility reconciliation, then emit at most one recovery summary. Recheck each pending item/group immediately before submission and suppress ineligible intents. First history import is unknown handledness, distinct from recovery of an enrolled gap. [Apple authorization][notifications] [Responses][notification-response]

**Non-goals.** Approving tools from notifications, terminal input injection, notification-database scraping or treating a delivered banner as seen/resolved.

**Tests.** Five concurrent root completions; thirty-action mixed queue; permission auto-approval versus real wait; exact request resolution; old-scope clear versus new wait; clear-before-delayed-positive; follow-up permutations; acknowledgement versus resolution; snooze/restart; notification denied/Focus-muted; UI quit; crash between submit/result; stale notification and changed tab/session.

**Quantified acceptance.** Five distinct completed root turns retain five addressable items through grouping and ten restarts. Thirty-action mixed fixture preserves every action and command after replay. Twenty native notification interactions route to the proper item/inspector or verified target; none auto-acknowledges lower-proof routing. No replay banner storm, no duplicate late output after already-recorded accepted follow-up, and no parent-owned child-output storm even after later root promotion. All queue operations are keyboard-accessible.

**Required evidence/artifacts.** Attention causal-order fixture matrix; native permission/settings captures; outbox failure ledger; notification/cold-start video; before/after persisted item counts; exact versus lower-proof acknowledgement tests.

**Exit gate.** Required owner actions remain available until their defined acknowledgement/resolution policy is satisfied, independently of banners, scene visibility and restarts.

## M6 — Codex with truthful runtime and routing modes

**Purpose.** Complete the local MVP with a second provider without manufacturing Claude-shaped semantics.

**Prerequisites.** M5; M0C Codex release/schema/runtime probes.

**Scope.** CODEX_EMBEDDED, CODEX_SHARED_DAEMON and separately qualified desktop-local profiles; native hooks; passive daemon observation; turn-history recovery; actor identity and explicit surface pairing.

**Implementation requirements.** Pin the researched `0.160.1` schema/source family. Map persistent `Thread.id`, tree/root `session_id`, child `agent_id`, `parentThreadId` and historical `forkedFromId` separately. Hook Stop is a response boundary. Optional legacy notify receives its original JSON argv argument and creates OUTPUT_READY; preserve any existing notifier through a safe dispatcher. [Codex schema][codex-schema] [Turn loop][codex-turn] [Notifier][codex-notify]

Probe the already-running daemon with read-only `codex app-server daemon version`; connect by WebSocket over AF_UNIX, send initialize/receive response/send initialized, and record the answering daemon version independently of the CLI. Begin bounded receiving before handshake/enumeration. Use an explicit limit/cursor for `thread/loaded/list`, metadata-only `thread/read` including historyMode, global status changes and qualified `thread/turns/list` with `itemsView: "notLoaded"`. Do not advertise unsupported interactive client capabilities. History availability belongs to that reachable daemon/store; embedded mode does not inherit it, and Threadspace never launches another app-server to obtain history. Page to the persisted anchor/exhaustion and revisit nonterminal turns. Coalesce/throttle requests with one history request in flight per daemon/store: legacy history rereads the rollout per page, so a small page does not bound provider CPU. Do not call resume/start, answer requests or subscribe by creating work. Native idle is not successful completion. SPEC Section 12.6 controls history authority: markerless legacy Completed shells and normalized Interrupted without terminal evidence are reconstructed status only. Qualify paginated Completed/Failed and recorded interruption against the exact local runtime/store. Legacy terminal outcomes require independent matching native terminal evidence; two timestamp markers alone cannot prove the event targeted that turn. Timestamp presence can encode a pinned native boundary; timestamp ordering and UUID shape are never native causal proof. [Daemon][codex-daemon] [Thread protocol][codex-thread] [History implementation][codex-history]

Input steering may retain an active native Turn ID. Submission does not create a new Turn by itself; a follow-up after a terminal outcome creates a new turn only through its actual native identity/start evidence.

Embedded ancestry earns exact surface plus NATIVE_BOUND_LAST_KNOWN unless an actual new native current-thread lookup qualifies. Shared daemon ancestry does not identify its originating TUI. Provide explicit `/status` UUID pairing with the label “Focus paired tab — current thread unverified.” In-place `/new`/resume cannot silently promote that historical assertion. Notifications open inspector/choice for lower-proof bindings. Offer `--no-daemon` only as an explicit mode choice that preserves user launch preferences.

**Non-goals.** Hook parity, inferred terminal errors/waits, automatic current-thread assurance from freshness, arbitrary observer task creation or requiring a launcher.

**Tests.** Shared-daemon and embedded CLI; two same-cwd clients; root/child identity; active-turn steering; Stop veto; retrying versus terminal error; aggregate waits; five recorded native offline outcomes in a previously enrolled qualified thread recovered with page size two; first history import with unknown handledness; markerless legacy Completed and normalized Interrupted negatives; native start A followed by an unmatched terminal B yielding a legacy A row with both markers; repeated cursor; unavailable embedded/ephemeral history; new thread during pagination; in-place switch/pairing invalidation. Separately complete five owner-facing threads without later accepted human follow-up. Measure legacy history request concurrency and CPU.

The native five-outcome recovery gate applies to each profile advertised as supporting recorded-history recovery. A legacy-only or unreachable store may pass M6 with that capability marked NOT_SUPPORTED after its reconstructed-history and coverage-gap tests pass. The synthetic five-outcome pagination fixture remains mandatory; it does not certify native recovery for a limited profile.

**Quantified acceptance.** Each enabled recorded-history recovery profile recovers all five recorded native historical outcomes; their attention respects any proven follow-up resolution. The five-thread no-follow-up case retains five unresolved output items. A one-thread synthetic multi-page fixture with no accepted-human-follow-up facts also retains five. Unmarked reconstructed shells create no native terminal outcome or automatic completion attention. Delayed concrete outcome evidence settles only its matching turn. No unbounded inventory/history request or duplicate notification replay. Thirty synthetic/native route attempts remain within their proven tier; zero guessed shared-daemon TUI route. Two same-cwd threads stay independent. Observer method trace contains zero task-start/resume/control operations. Ten reconnects preserve identity, attention and honest coverage gaps.

**Required evidence/artifacts.** Per-mode capability reports; native protocol/hook fixtures; read-only method audit; pagination ledger; same-cwd proof; paired-tab and in-place-switch videos; installed CLI/desktop version distinctions.

**Exit gate — MVP.** M0A–M0C and M1–M6 pass together: externally launched Claude with current verified Terminal return, stable lifecycle/actors, durable attention, restart recovery, and useful Codex integration with explicit mode-specific limits. The minimal office and operational list are usable without final artwork.

## M7 — Project, repository and worktree world

**Purpose.** Turn reliable workers into a persistent spatial map of actual work.

**Prerequisites.** M6. Preserve working MVP behavior throughout layout work.

**Scope.** Project/Repository/Worktree identity; filesystem continuity; Git context; stable rooms/desks; layout persistence and owner overrides.

**Implementation requirements.** Query Git metadata with bounded read-only argv operations. Persist random identities, canonical paths/aliases and qualified filesystem evidence. Linked worktrees share a repository/project; separate clones do not merge because remote URLs match. Branch is mutable. Non-Git roots and unavailable volumes remain valid explicitly represented cases. [Git paths][git-paths] [Worktrees][git-worktrees]

Persist native bookmarks and revalidate continuity after restart; fileResourceIdentifier alone is not persistent across system restarts and cannot be archived as cross-boot identity authority.

Keep home Project distinct from current cwd/additional roots. Directory changes do not rekey sessions or silently move their home. Reuse reserved desks; append new stable slots without repacking others. Persist camera/layout/avatar assignment independently of execution presence. Explicit auto-layout and owner moves have undo.

**Non-goals.** Git writes, branch/worktree creation, repository merging by URL, or installing provider worktree-override hooks.

**Tests.** Two repositories, three linked worktrees, one independent clone, non-Git directory, detached HEAD, rename, delete/recreate path, unmounted volume, cwd switch and twenty session resumes.

**Quantified acceptance.** All fixture identities remain correctly distinct; twenty restarts reproduce identical owner layout and desk assignments. A deleted/recreated directory cannot inherit the old repository solely from its path. Existing sessions/attention survive every context change; no unrelated worker moves during new-slot allocation.

**Required evidence/artifacts.** Filesystem/Git identity fixtures; project-context lineage; saved layout/replay hashes; native multi-project recording; credential-redaction checks for remote URLs.

**Exit gate.** Project places and worktree clusters remain stable through normal changes while session identity and routing remain independent.

## M8 — iTerm2, Ghostty and tmux surfaces

**Purpose.** Extend precise return to other existing terminal workflows using their actual native contracts.

**Prerequisites.** M7; installed-version/dictionary qualification for every advertised adapter.

**Scope.** iTerm2 pane/session identity and AppleScript focus; Ghostty capability-probed AppleScript; tmux pane plus selected outer client; optional explicit surface registration.

**Implementation requirements.** iTerm2 selects the session/pane, containing tab and window, then verifies focus. A URL reveal without readback is only URL_DISPATCHED. Ghostty native AppleScript UUID and `GHOSTTY_SURFACE_ID` are different namespaces. Probe the installed dictionary for `tty`/`pid`; merged source is not proof a release contains them. Without passive reverse mapping, require explicitly linked native UUID or offer a lower routing tier. [iTerm scripting][iterm] [Ghostty scripting][ghostty] [TTY/PID change][ghostty-tty]

tmux binds server ProcessKey/socket, pane ID/TTY and specific attached client/outer TTY. `pane_pid` is the initial process, not foreground identity. Select the chosen client without detaching or stealing another client. Multiple eligible clients require choice/pin; detached panes have no current outer surface. Never treat a remote collector's connection as the work surface. [tmux manual][tmux]

**Non-goals.** Universal terminal heuristics, invented Ghostty ID conversion, implicit mux client choice or unqualified nested tmux/SSH chains.

**Tests.** Split panes, reordered/moved tabs, minimized windows, app restart, stale IDs, foreground/background jobs, missing permission; Ghostty with/without required properties; tmux two clients, client detach/reattach, server restart and pane reuse.

**Quantified acceptance.** At least twenty native routes per advertised adapter/profile with zero wrong target; ten ambiguity/stale cases cause no unintended focus. A reused pane/native ID cannot revive another generation. Every successful result records native surface, current-session proof ceiling and input readiness separately.

**Required evidence/artifacts.** Dictionary/API hashes; native route/readback matrix and recordings; mux server/client/pane proof; unsupported-profile UI captures.

**Exit gate.** Each advertised terminal mode earns its documented tier; unavailable native properties produce honest degraded behavior without weakening existing Terminal.app guarantees.

## M9 — Session-scoped semantic MCP

**Purpose.** Add useful task meaning without giving model-written text control over observed lifecycle.

**Prerequisites.** M6 and fresh native current-session verification in CLAUDE_NATIVE_OBSERVER. M8 is independent.

**Scope.** Stdio `threadspace-mcp`, version-pinned official SDK/protocol and connection enrollment. Implement `threadspace_set_task_title`, `threadspace_set_phase`, `threadspace_report_checkpoint`, `threadspace_request_attention`, `threadspace_report_blocker` and `threadspace_declare_handoff_ready`.

**Implementation requirements.** Initially enable automatic enrollment only for qualified Claude with fresh current-session lookup. Enrollment binds endpoint, namespace, Session, proven Actor/activation, expiry and allowed operations. Revalidate before mutations; a session switch invalidates enrollment. Neither shared-daemon ancestry nor embedded Codex's last-known binding qualifies. A later provider needs its own currentness proof. Keep enrollment immutable per stdio connection: never silently retarget A to B. Invalidate and refuse until a newly proven connection/call scope exists. Leave Actor unspecified unless independently proven; a shared root/child MCP process does not identify its caller. Tokens stay outside model arguments/prompts.

Journal `SEMANTIC_ANNOTATION_RECORDED` with `SEMANTIC_SELF_REPORT` provenance and scoped idempotency. Apply 8 KiB call, 120-character title, 1,000-character summary, twenty-reference/checklist-entry limits; rate limit ten calls/second with burst twenty. Semantic blocker/phase never changes native WAITING, outcome or execution presence. [MCP specification][mcp]

**Non-goals.** Lifecycle hooks through MCP, approving tools, provider prompt submission or treating reported tests as independently verified.

**Tests.** All six tools; repeated/conflicting keys; hundred session-switch/reload enrollment races; wrong actor/endpoint; daemon ambiguity; oversized data; unavailable companion; semantic “finished,” “blocked” and “tests pass” alongside contradictory native state.

**Quantified acceptance.** No invalid enrollment mutates state; repeated valid key produces one annotation/action; every tested semantic claim preserves native state. Outage returns a bounded tool error and does not stop native observation.

**Required evidence/artifacts.** Exact SDK/protocol lock; tool schemas; enrollment audit fixtures; semantic/native conflict matrix; rate/size results.

**Exit gate.** Meaningful annotations and explicit owner requests are useful, correctly scoped and incapable of masquerading as native lifecycle facts.

## M10 — Remote and browser groundwork

**Purpose.** Support remote identities and original working URLs without importing local-TTY assumptions.

**Prerequisites.** M6; second qualified macOS endpoint with explicit collector setup and existing host-key-verified SSH access. M8/M9 are independent, and missing remote hardware does not block local release.

**Scope.** Mac-initiated outbound SSH collector; remote durable spool/ACK protocol; endpoint enrollment; remote project links; original provider URL registration and native dispatch. [OpenSSH client][ssh]

**Implementation requirements.** Run the fixed `threadspace-remote serve --protocol=1` command over noninteractive SSH stdio. Use existing SSH configuration without copying private keys or bypassing host verification. Remote capture returns REMOTE_SPOOLED only for a durably admitted record, distinct from MAC_COMMITTED after the Mac journal transaction. Retain admitted unacknowledged records; seven days is an overdue warning, not silent expiry. At 256 MiB or 100,000 records reject new durable admissions while hooks still fail open; owner discard records a loss. Preserve observation UUIDs and persisted relay-epoch/sequence identity. The Mac atomically stores a contiguous receipt watermark and retains it through compaction so an old retried prefix cannot recreate facts after ordinary dedup history expires. Bound batches to 1 MiB and frames to 64 KiB.

Namespace identical provider IDs/PIDs/TTYs by endpoint. Record lag and coverage; disconnection never proves completion. The collection SSH connection is not the original task terminal. Remote URL dispatch has its own proof tier. Define the future opted-in browser profile/tab/URL contract; exact browser focus requires actual revalidation.

**Non-goals.** Hosted backend, public listener, silent remote installation, Windows/Linux parity or automatic SSH-pane correlation.

**Tests.** Two endpoints with colliding native IDs/PIDs; thousand-record partition/reconnect; remote/Mac restart; repeated/lost ACK; outage beyond seven days; quota rejection without deleting admitted pending records; replay after Mac journal compaction; clock skew; endpoint reinstall; host verification failure; original URL unavailable and stale browser locator.

**Quantified acceptance.** All durably accepted recoverable records arrive once semantically; zero cross-endpoint merges or local-TTY matches. Lost network never produces native terminal outcomes. Incorrect endpoint identity stops forwarding; overflow is visible rather than unbounded.

**Required evidence/artifacts.** Second-Mac native trace; enrollment/config diff; reconnect/receipt ledger; bounded-spool results; URL proof labels.

**Exit gate.** One actual remote profile works through its qualified transport; browser groundwork exposes truthful URL behavior and no fabricated exact-tab claim.

## M11 — ChatGPT experimental integration

**Purpose.** Deliver a useful linked ChatGPT worker while containing unsupported lifecycle assumptions.

**Prerequisites.** M6, the shared endpoint/URL-surface contract and explicit experimental enablement. M10's second-machine collector qualification is independent.

**Scope.** Original conversation URL, owner-provided title/project, manual semantic attention and native URL/app routing. Local-only Work/Codex profiles remain distinct from consumer Chat and personal cloud Work.

**Implementation requirements.** Display actual capability tier. Reject share links as original-work locators; do not invent private deep links. Consumer lifecycle stays unknown without a qualified source. An optional selected-tab/window DOM/AX experiment remains UI_INFERRED, explicitly scoped and separately disabled when hidden, suspended or structurally unrecognized. Work with Apps and native notification availability are not outbound lifecycle APIs. [Shared links][chatgpt-shared] [Work with Apps][chatgpt-apps]

**Non-goals.** Private database/cookie/endpoint inspection, notification database access, full ChatGPT parity or making this experiment an MVP prerequisite.

**Tests.** Original versus share URL; multiple accounts/profiles; tab closure/replacement; URL routing preference; manual completion/resolution/restart; unsupported source and optional UI structure/localization changes.

**Quantified acceptance.** Twenty linked-worker operations preserve the correct record and explicit tier; zero UI inference upgrades to native lifecycle; closing a tab never creates completion/cancellation. Unsupported experiment fails closed while manual operations remain usable.

**Required evidence/artifacts.** Current capability/source report; original-URL route recording; experimental flag/permission behavior; inference provenance fixtures.

**Exit gate.** Linked ChatGPT work is useful and honestly labeled; unsupported native lifecycle remains a visible boundary, not an implementation claim.

## M12 — Visual, native and accessibility polish

**Purpose.** Make the trusted substrate feel like a coherent macOS office and efficient daily utility.

**Prerequisites.** M7; frozen correctness suite and existing office/2D controls. Include already-advertised extras; unavailable M8–M11 profiles do not block local polish.

**Scope.** Extensible CharacterSkin/Avatar system; stable identity; state animations; project rooms; relationship selection; attention drawer, fleet, inspectors and keyboard switcher; native chrome/menus; dark/light and accessibility.

**Implementation requirements.** Keep one imperative SceneController over provider-neutral view models. Pool/instance repeated geometry; preserve animation interpolation independently of event cadence. Use pinned WebGPURenderer/TSL and current RenderPipeline/resource lifecycle APIs; dispose assets without leaking GPU objects. [Three renderer][three] [Pinned source][three-source]

Retain a geometric test skin and add replaceable production skins without freezing a creature choice into identity. Distinguish working, input, approval, outcome, stale coverage, subordinate run and true departure through text/icon/motion as appropriate. A completed principal stays; an ended principal leaves an attention marker where needed.

Native traffic lights/titlebar, safe drag regions, fullscreen, window restoration and standard menus behave consistently. Every action has a DOM/keyboard equivalent. Reduced motion disables idle loops/camera easing, and colors have textual equivalents. For the pinned Three renderer, confirmed hidden/minimized or 2D-only mode disposes the renderer and its internal RAF work while retaining CPU scene state; visible return uses a fresh initialized/attested renderer. Generation checks prevent hide-during-init or late loss callbacks reviving it. Capture continues.

**Non-goals.** Physics/game navigation requirements, decorative features that delay owner actions or a second independent frontend truth model.

**Tests.** All canonical visual states; two interchangeable skins; five sequential attention actions without canvas navigation; VoiceOver/keyboard; reduced motion; increased contrast; light/dark; DPR and monitor changes; GPU loss/rebuild with injected versus actual evidence identified; hide during initialization; repeated hide/show disposal; scene hidden while providers continue.

**Quantified acceptance.** One hundred sampled view revisions agree across DOM, scene and canonical state. All operational actions work with canvas disabled and no pointer. No keyboard trap or state conveyed by color alone. A single bounded GPU rebuild preserves attention and native routing; repeated failure retains usable 2D controls.

**Required evidence/artifacts.** Native visual-state atlas; presented-pixel recordings; keyboard/VoiceOver checklist; skin interface/assets; resource lifetime counters; user-flow recordings.

**Exit gate.** The office improves comprehension and remains native, accessible and operational when graphics are unavailable.

## M13 — Performance, durability and reliability qualification

**Purpose.** Prove bounded resource use and long-running truth on the intended Mac.

**Prerequisites.** M12. Benchmarks use the locked actual package, not a browser-only build.

**Scope.** Profiling, targeted optimization, retention/checkpoints, crash/update/reconnect stress, native presentation and provider-independent load.

**Implementation requirements.** Use the canonical workload: 32 principal sessions, 64 subordinate actors, eight projects, at most 64 visible animated avatars, 1440×900 logical viewport and capped DPR 1.5. Capacity workload is 100 principals, 300 subordinates and twenty projects with the same visible-avatar cap. Transport stress is 200 observations/second for fifteen minutes plus a 2,000/second five-second burst. Record payload distribution, warmed and cold-cache runs separately. Measure the companion, Tauri UI, WebKit content and GPU process family; Rust RSS alone is insufficient.

Keep queues, snapshots, history pages, spool and assets bounded. Preserve every accepted owner action while coalescing only presentation. Checkpoint before journal compaction and retain identity, native dedup/coverage anchors, unresolved attention and owner commands. Enforce the 30-day/1 GiB ordinary detailed-history budget while separately retaining/reporting protected state, causal/dedup evidence, checkpoints and database/WAL/backup files; short readers cannot wait on UI/OS/ACKs. Enforce seven-day/256 MiB local capture-spool budgets; remote admitted records follow the no-silent-expiry receipt policy in M10; corruption recovery preserves the original store and reports unrecovered coverage.

**Tests and quantified acceptance.**

| Measurement | Required target |
| --- | --- |
| Normal capture execution | p95 ≤25 ms; application budget 250 ms, shutdown path 100 ms; 20 ms connect/75 ms receipt budgets |
| Local capture → committed state | p95 ≤100 ms under normal load |
| Committed state → applied native UI | p95 ≤100 ms under normal local load |
| Native event → visible state/scene | p95 ≤250 ms on the healthy normal path; report provider scheduling separately |
| Return-to-Agent | p95 ≤750 ms for qualified normal Terminal route with permissions granted; attempt deadline two seconds; report failure tiers separately |
| Animated canonical office | Target 60 FPS; p95 frame interval ≤20 ms over five warmed minutes; no sustained input stall above 100 ms |
| Quiescent companion / unchanged visible UI | ≤1% / ≤3% of one logical CPU over five minutes; animated workload reported separately |
| Warm memory | Companion ≤100 MiB quiet/≤150 MiB normal stress; UI/WebKit/GPU family ≤600 MiB normal/≤1 GiB capacity |
| Retained memory growth | <20 MiB after eight-hour equivalent create/dispose/reconnect workload and stabilization |
| Startup reconstruction | Attention usable ≤2 seconds from core readiness on 100,000-event store; warm scene usable ≤4 seconds from UI launch |
| Spool catch-up | 50,000 typical observations drained within 60 seconds without blocking new capture |
| Hidden scene | Zero internal scheduled Three animation work after disposal; native capture and attention remain functional; fresh generation rebuild on return |
| Correctness during faults | Zero wrong routes, duplicate identities, false terminal outcomes or lost durably accepted owner actions |

Run a 24-hour synthetic soak with periodic authoritative replay comparisons; a 100,000-observation replay; bursts beyond the live subscription window; stalled/missing Channel frame; oversized snapshot; stale paged query; one hundred UI reload/close cycles; twenty helper crash/ACK injections; ten actual sleep/wake cycles. Test disk-full/spool-full, permissions revoked, corrupted checkpoint, interrupted migration, obsolete schema and overlapping old/new helper startup.

Exercise native changing pixels during IPC/resource cancellation and hidden/wake. Upstream Wry reports motivate these cases but are not assumed reproductions in this version. A live DOM/AX tree cannot alone certify a displayed frame. [Scheme race][wry-race] [Presented-frame report][wry-frames]

**Non-goals.** Optimizing unmeasured bottlenecks, weakening FULL durability to achieve a benchmark or suppressing visible uncertainty to appear faster.

**Required evidence/artifacts.** Raw samples and percentile calculations; Instruments/process-family measurements; workload seed/database size; 24-hour state hashes; fault ledger; native frame captures; memory/resource trends; documented bounded fixes.

**Exit gate.** Targets pass on the recorded target configuration, or a material architecture/budget change is explicitly approved and reflected in SPEC. No unexplained correctness failure or sustained resource growth is accepted.

## M14 — Original end-to-end sequence and adversarial acceptance

**Purpose.** Prove the complete intended experience and repeat it under the failures most likely to expose false confidence.

**Prerequisites.** M13; all advertised profiles qualified; clean test projects; instrumented evidence capture plus an uninstrumented installed candidate.

**Scope.** The complete original 31-step sequence below, native provider evidence, repeated independent runs and a consolidated adversarial suite. Earlier milestone evidence does not replace this final integrated run.

**Implementation requirements.** Build an acceptance runner/checklist that records canonical/native IDs, source facts, route proof and expected state at each step. Use deterministic synthetic tests for exhaustive failure permutations and real providers for integration truth. Model-generated text saying a test passed is not evidence.

### Original 31-step acceptance matrix

| Step | Action or observation | Required result / principal evidence |
| --- | --- | --- |
| 1 | Threadspace runs as a native macOS application. | Exact installed Tauri 3 build and real WebGPU diagnostics. |
| 2 | Manually open Terminal.app outside Threadspace. | Independent native tab; no owned PTY/launcher. |
| 3 | Manually launch Claude Code. | Ordinary user launch with integration already enabled. |
| 4 | Threadspace detects the session automatically. | Native identification/registration appears without manual session creation. |
| 5 | Associate Claude, provider session ID, project/repository and source surface. | Recorded canonical/native join and live process/TTY proof. |
| 6 | A worker enters the correct project area. | Stable worker and correct initial home Project. |
| 7 | Submit a prompt. | Native InputAttempt with original provenance. |
| 8 | Worker becomes WORKING. | Native turn start/step, same Session. |
| 9 | Claude uses tools. | Actual provider tool activity. |
| 10 | Threadspace reflects real activity without terminal-text scraping. | Hook/mod facts and view revision agree. |
| 11 | Claude spawns a subagent. | Native actor/spawn evidence. |
| 12 | Another worker appears with the correct parent. | Stable child identity and immediate-parent proof. |
| 13 | The subagent completes. | Qualified native child-run outcome. |
| 14 | The subagent leaves or resolves. | Active child presentation ends; history remains; no routine owner notification. |
| 15 | The main Claude turn completes. | Native terminal turn outcome. |
| 16 | The main worker STAYS in the office. | Execution remains live; same worker/desk. |
| 17 | The main worker signals owner attention. | One durable turn-output item and readable marker. |
| 18 | A native macOS notification appears. | Actual OS banner under permitted settings, recorded request/item ID. |
| 19 | Click the worker or attention item. | Selection opens the inspector; explicit Return/double-click invokes the shared native router. |
| 20 | Return to the exact Terminal.app tab containing that Claude session. | Fresh provider mapping, tab readback, current-session proof and frontmost result. |
| 21 | Submit a follow-up prompt. | Record the actual input and its qualified origin/acceptance; claim original-after-output causality only with the profile's positive witness. |
| 22 | Handle the existing attention item. | With qualified original causality, the eligible prior output auto-resolves. Otherwise it remains outstanding until explicit Mark handled, whose durable command resolves it. Record the unavailable automatic facet honestly; unrelated actions remain. |
| 23 | THE SAME session/worker returns to WORKING. | New native Turn, unchanged canonical Session/avatar. |
| 24 | Start another Claude session in the same repository. | Distinct native session ID in the same project/cwd. |
| 25 | Both sessions appear independently. | Two workers; neither completion/routing changes the other. |
| 26 | Start a session in another project. | Independent identity and repository context. |
| 27 | It appears in that project's own spatial area. | Correct separate room/home. |
| 28 | End one real Claude session. | Actual matched execution-end evidence. |
| 29 | Only now does its principal worker leave. | Departure follows execution closure; unresolved attention survives. |
| 30 | Restart Threadspace and validate reconstruction. | Identity, active workers, layout and attention restored without duplicate entrances. |
| 31 | Repeat the essential lifecycle using Codex. | Each enabled mode proves its actual identity/outcome/wait/routing tier; no invented hook parity or current-TUI certainty. |

### Adversarial matrix

| Scenario | Required invariant |
| --- | --- |
| Fifteen concurrent sessions, five same repo, several worktrees | Independent identity/turns; stable context and correct routes. |
| Three same-repo sessions, complete one and return to each | Only the intended turn changes; each requested native target is verified. |
| Resume elsewhere, simultaneous attachments, A→B→A | One durable worker; explicit attachment choice; late callbacks retain original ownership. |
| Late launch, UI force-quit, helper crash, sleep/wake | Reconstruction respects durability/coverage; no duplicate or fabricated current state. |
| PID/TTY/native ID reuse, tab moves, rapid close | Old binding rejected; no arbitrary same-cwd focus. |
| Duplicate, delayed, out-of-order and missing event | Idempotence/causal convergence; repair only with sufficient evidence. |
| Multiple children; child outlives parent turn | Correct immediate parent and current run; no cascade disappearance. |
| Several outputs while unfocused; snooze/ack/restart | Every owner item preserved under its specific policy. |
| Queued-before-output, upstream-delayed, blocked or automatic prompt; accepted input replayed before its earlier output | No inappropriate auto-resolution; later-arriving eligible prior output resolves from retained causal evidence. |
| Codex daemon clients, offline pages, in-place thread switch | Honest coverage and routing ceiling; no guessed originating TUI. |
| Remote lacks local TTY; ChatGPT lacks native lifecycle | Endpoint/URL/manual tiers remain useful without fabricated bindings/outcomes. |
| Stalled Channel, old native view under reused label, oversize snapshot/page race | Public native-incarnation validation, independent recovery, bounded cache memory, atomic hydration and no stale overwrite. |
| Notification click after changed target or backgrounded foreground job | Revalidate current binding/readiness; no lower-proof automatic acknowledgement. |
| Markerless Codex history or recovery-normalized interruption | Useful history status without fabricated native outcomes or completion attention. |
| Renderer/WebView failure or hide during GPU initialization | Capture survives; no late hidden renderer revival; bounded fresh-view/renderer recovery. |

**Non-goals.** Declaring a failed external integration passed because its synthetic adapter works, hiding a routing limitation, or counting output completion as session termination.

**Tests and quantified acceptance.** Complete three consecutive native 31-step runs; one includes cold UI startup after background capture and another includes a restart with external work continuing. Run all mandatory synthetic adversarial cases across 10,000 recorded seeds. Zero wrong-session routes, identity conflations, lost accepted attention or false session departures. Step31 uses separate embedded/shared-daemon evidence and visibly retained reduced capabilities where specified.

**Required evidence/artifacts.** Per-step pass matrix with journal/native references; uncut native recordings; test build hashes; route readbacks; replay seeds and invariant results; open limitations by profile.

**Exit gate.** All original steps and required adversarial invariants pass in the integrated product. A known external boundary may qualify only the explicitly limited profile already defined in SPEC. In particular, step22's explicit handling path can pass while the unavailable automatic-resolution facet remains NOT_SUPPORTED; it cannot excuse failure of primary Claude identity, native outcomes or exact current-session Return.

## M15 — Packaging, setup, updates and release

**Purpose.** Deliver a reproducible install that preserves the proven behavior outside the development environment.

**Prerequisites.** M14 and the M0A/M0C-qualified stable local signing identity. Developer ID/notarization credentials are required only for the optional public-distribution branch.

**Scope.** Personal arm64 release; inside-out native signing; background registration; reversible setup/removal; update/schema handshake; diagnostics and documentation. Public distribution adds Developer ID/notarization qualification.

**Implementation requirements.** Pre-sign companion/hook assets, copy the complete companion app under `Contents/Library/LoginItems` and sign the outer application without later mutation. The outer native bootstrap owns the one login-item registration; no custom LaunchAgent job or parallel registration is shipped. Personal installation uses the qualified stable local identity and tested macOS trust/permission flow. Public distribution additionally requires Developer ID signing, notarization and stapling of the final artifact. Verify entitlements, paths and executable identities. Ship no qualification server, synthetic injection command, debug capability, test-only permission or Tauri 2 dependency.

Setup previews scoped changes, backs up provider configuration and installs owned handlers idempotently. Explicit Enable may start the sole companion control-only; it authenticates bootstrap, verifies service authorization, performs any authorized migration and journals the enable preference before admitting capture. Registration or notification activation alone never enables observation. Recheck file hashes before writes; conflicts cannot overwrite user changes. Stop Observation quiesces accepted work before the outer bootstrap unregisters supervision and verifies termination. Uninstall removes still-matching owned changes only; retaining local history is the default, deletion explicit. Updates preserve the observation-enabled preference and use SPEC Section 19.5's recorded maintenance transaction. While still supervised, the companion gates new admission, finishes accepted work, creates a consistent backup, reports PREPARED and stays alive holding its lock. The outer bootstrap then unregisters, which can terminate the helper; no required drain or backup follows that call. Abort replacement on preparation/unregistration/exit uncertainty. Verify old ProcessKey exit/lock release and replace the bundle. If observation was enabled, authorize the recorded migration target and register/start the updated companion quiescent; that companion alone acquires the writer lock, validates/migrates, reports ready, and admits capture after maintenance completes. If observation was disabled with no writer, verify service/lock state, replace without a nonexistent PrepareMaintenance response, and defer migration until the next explicit supervised enable. The outer bootstrap never writes SQLite. A restart during pending maintenance remains quiescent until verified completion/cancellation; bootstrap recovers the recorded phase after interruption. Unsupported downgrade opens recovery rather than writing a second store.

**Non-goals.** App Store distribution, Intel certification, auto-publishing an update or silently installing remote integrations.

**Tests.** Clean-user installation; first permissions; denial then enablement; source checkout absent; second app launch; UI quit/background work; login; update with active sessions; old/new helper overlap; failed preparation/unregistration; helper crash/relaunch during pending maintenance; interrupted update; observation-disabled second launch; explicit re-enable after disabled upgrade; notification-only activation while disabled; uninstall/reinstall with unrelated hooks preserved.

**Quantified acceptance.** Three clean install/update/uninstall cycles preserve unrelated configuration byte-for-byte where untouched; no duplicate service/writer/handlers. Every durably accepted record survives a normal upgrade; unsupported schema cannot be written by the old helper. Verify inner/outer signatures; repeat essential M0 native smoke and one M14 sequence on the exact personal release. Public distribution separately verifies notarization/stapling and its signed artifact.

**Required evidence/artifacts.** Reproducible build recipe; lockfiles/platform manifest; signed artifact hashes; nested signature/entitlement report; install diffs/backups; upgrade/recovery ledger; compatibility matrix; setup/troubleshooting guide. Add notarization evidence only for the public branch.

**Exit gate.** The personal release passes native smoke/lifecycle acceptance, installs/removes predictably and states truthful support limits. Public distribution remains unavailable until its additional gate passes; paid distribution credentials are not a personal-product prerequisite. Release approval refers to the concrete artifact and evidence manifest.

## Primary implementation references

Links support API/version facts, not executed native proof. SPEC.md contains the broader source register and interpretation boundaries.

[tauri-release]: https://github.com/tauri-apps/tauri/releases/tag/tauri-v3.0.0-alpha.4
[rust]: https://github.com/rust-lang/rust/releases/tag/1.99.0
[three-package]: https://github.com/mrdoob/three.js/blob/9b4a2ac29c63ccb43fd51c5661f2f873ac2c39b8/package.json
[sqlite-release]: https://sqlite.org/releaselog/3_53_4.html
[claude-release]: https://github.com/anthropics/claude-code/releases/tag/v2.1.290
[codex-release]: https://github.com/openai/codex/releases/tag/rust-v0.160.1
[tauri-manifest]: https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/Cargo.toml
[tauri-channel]: https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri/src/ipc/channel.rs
[tauri-tests]: https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri/src/test/mod.rs
[tauri-bundler]: https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri-bundler/src/bundle/macos/app.rs
[plugins]: https://github.com/tauri-apps/plugins-workspace/blob/d9be6d0492fb6746637ba64497237b2116aaec90/Cargo.toml
[wdio]: https://github.com/webdriverio/desktop-mobile/blob/fb13c9343a24c8d45d396f2258698f49f34ec435/packages/tauri-plugin-webdriver/Cargo.toml
[service]: https://developer.apple.com/documentation/servicemanagement/smappservice
[claude-hooks]: https://code.claude.com/docs/en/hooks
[claude-mods]: https://code.claude.com/docs/en/plugins/mods/reference
[claude-inventory]: https://code.claude.com/docs/en/agent-view
[claude-teams]: https://code.claude.com/docs/en/agent-teams
[codex-schema]: https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/hooks/src/schema.rs
[codex-turn]: https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/src/session/turn.rs
[codex-notify]: https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/hooks/src/legacy_notify.rs
[codex-daemon]: https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server-daemon/README.md
[codex-thread]: https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server-protocol/src/protocol/v2/thread.rs
[codex-history]: https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server/src/request_processors/thread_processor.rs
[notifications]: https://developer.apple.com/documentation/usernotifications/asking-permission-to-use-notifications
[notification-response]: https://developer.apple.com/documentation/usernotifications/handling-notifications-and-notification-related-actions
[git-paths]: https://git-scm.com/docs/git-rev-parse
[git-worktrees]: https://git-scm.com/docs/git-worktree
[iterm]: https://iterm2.com/documentation-scripting.html
[ghostty]: https://ghostty.org/docs/features/applescript
[ghostty-tty]: https://github.com/ghostty-org/ghostty/pull/11922
[tmux]: https://github.com/tmux/tmux/blob/master/tmux.1
[mcp]: https://modelcontextprotocol.io/specification
[ssh]: https://man.openbsd.org/ssh
[chatgpt-shared]: https://help.openai.com/en/articles/7925741-chatgpt-sharedlinks-faq
[chatgpt-apps]: https://help.openai.com/en/articles/10119604-work-with-apps-on-macos
[three]: https://threejs.org/docs/pages/WebGPURenderer.html
[three-source]: https://github.com/mrdoob/three.js/tree/9b4a2ac29c63ccb43fd51c5661f2f873ac2c39b8
[wry-race]: https://github.com/tauri-apps/wry/issues/1822
[wry-frames]: https://github.com/tauri-apps/wry/issues/1848

[service-login]: https://developer.apple.com/documentation/servicemanagement/smappservice/loginitem(identifier:)
[service-register]: https://developer.apple.com/documentation/servicemanagement/smappservice/register()
[service-unregister]: https://developer.apple.com/documentation/servicemanagement/smappservice/unregister()
