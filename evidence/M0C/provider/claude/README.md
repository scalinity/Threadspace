# M0C — Claude observer-mod qualification (Claude Code 2.1.291)

Provider-semantics qualification of the pinned observer mod (`packages/provider-mod`) against the installed Claude Code build, for the M0C "Provider semantics" paragraph and SPEC §5.2, §7.3, §8.2–8.4 and §11.4. The capability profile this evidence supports is `docs/compatibility/claude-observer-2.1.291.json`.

## Build under test

| Item | Value |
| --- | --- |
| `claude --version` | `2.1.291 (Claude Code)` |
| Executable | `~/.local/bin/claude` → `~/.local/share/claude/versions/2.1.291` |
| `$.session.version()` (native) | `{ version: "2.1.291", base: "2.1.291", builtAt: "2026-10-06T02:24:19Z" }` |
| Declarations (bundled skill, API + built-in tools) | `/private/tmp/claude-501/bundled-skills/2.1.291/3b3977b7073999b09c5c539a1be6e8c1/plugin-authoring/types/claude-code.d.ts` |
| — sha256 / lines / first line | `03815831f5ceaffc9557ebcd75fe8c3bf36369a404a3153865a3e920cb818d61` / 20,755 / `// Written by Claude Code 2.1.291.` |
| Declarations the engine lays beside a loaded mod | `<mod>/.claude-plugin/types/claude-code/index.d.ts` (API only; tools are in `claude-code-tools/`) |
| — sha256 / lines / first line | `c05419d753bc81d991ddec4b66af17a9172f498e59d28fad07a8e676635c07ce` / 15,507 / `// Written by Claude Code 2.1.291.` (identical in all three native runs: `native/run*-generated-types.json`) |
| Mod under test | `packages/provider-mod`: recorded test label `160641b` is unresolvable in the published history; published source `9b8d604c269ccc79ca8a728c5fd2b01dcd781a34` has the exact qualified file hashes (not an asserted commit-rewrite mapping). `hooks/register.ts` sha256 `f9a42b2522fd088ff2b7ba535adad75e1bbd523a04e8729532efb652c295b009`, `hooks/delivery.ts` sha256 `109251fcefe5f74914f9c598e9b63de2820a429b5c9b896aa2df821fe33300ca` |

Neither declaration file is committed (licensing); the hashes and the excerpts below identify them. Line numbers in this file refer to the bundled 20,755-line file.

## Evidence classes

- **engine test kit** — `claude plugin test packages/provider-mod`. Each test holds the engine's own `$` (its calls carry `next.origin = { plugin: "engine", tier: "core" }`) and an `on` whose hooks sit beneath every plugin and stand for the engine. In `next.trace` those stand-in hooks appear as `{ plugin: "test", tier: "builtin" }`, never as `{ plugin: "engine", tier: "core" }`; the kit therefore cannot show a real core link, and a native run supplies it. Inline plugins (`test(name, { plugins })`) load at a named tier in environments of their own.
- **native session** — `claude -p --model haiku --plugin-dir <disposable copy of the mod>` with a trivial prompt. Three runs (of the five allowed), each from a disposable directory under `/private/tmp/claude-501/`, removed afterwards. The capture argv reached the copy as its manifest default; no Claude settings file was read or written for it (the debug log states "no pluginConfigs[…] in user, --settings or managed settings … every option is its default").
- **declaration** — a quoted line of the 2.1.291 declarations.

A stand-in replaces the capture helper in both classes, because `threadspace-hook mod-batch` does not exist before M1/M2: a test `process.run` hook in the kit, and `harness/helper.py` (Python, `/usr/bin/python3 -I`) natively.

## Commands

```text
claude --version
claude plugin validate packages/provider-mod            → validate-output.txt   (exit 0)
claude plugin validate --json packages/provider-mod     → validate-output.json  (exit 0)
claude plugin test packages/provider-mod                → test-output.txt       (exit 0: 33 pass, 0 fail)
```

Native runs (every one: `--model haiku --no-session-persistence --setting-sources project --strict-mcp-config --plugin-dir <copy>`):

```text
run 1  claude -p … --tools "" --output-format json "Reply with exactly the word: ok"
run 2  CLAUDE_CODE_PLUGIN_DIR_WATCH=1 claude -p … --tools "" --input-format stream-json --output-format stream-json --verbose
         turn "Reply with exactly the word: one"; append a comment line to the copy's hooks/register.ts; wait ~21 s; turn "… two"; close stdin
run 3  claude -p … --tools Agent --output-format json "Call the Agent tool exactly once with subagent_type general-purpose, description 'say ok' and prompt 'Reply with exactly the word: ok. Use no tools.' Then reply with exactly the word: done"
```

The only edit to saved output is the repository's absolute path, written `<repo>`, and the home directory, written `~`.

### Re-running

The harness that produced this evidence is in `harness/` (qualification-only; it is not part of the mod). Each script takes absolute paths; run with `python3 -I`.

- `run_qual.py <repo> <out-dir>` — the three commands above, raw output to `<out-dir>`.
- `mutate.py <mod> <scratch-dir>` — applies each listed defect to a scratch copy and runs `claude plugin test` on it (`mutation-check.txt`).
- `native.py setup <mod> /private/tmp/claude-501/ts-m0c-claude-native-<run>` → `native.py run1|run2|run3 <run-dir>` → `native.py collect <run-dir> <out-dir> <label>` → `native.py cleanup <run-dir>`. `setup` copies the mod without its tests and sets the copy's `captureArgv` default to `helper.py`; it never writes Claude settings. Each `run*` is one `claude -p --model haiku` process. `analyze_run2.py <run-dir> <out.json>` derives the reload figures; `summarize.py <batches.jsonl>` prints a run's records.
- Afterwards remove the `~/.claude/projects/-private-tmp-claude-501-ts-m0c-claude-native-<run>-work` folder each run leaves (see Native session runs).

## Results

| # | Item | Test (kit) / run (native) | Result | Class |
| --- | --- | --- | --- | --- |
| 1 | Observer calls `next(e)` exactly once | `middleware contract > observer calls next(e) exactly once` | PASS | engine test kit |
| 2 | Event passed unchanged | `middleware contract > event passed unchanged` | PASS | engine test kit |
| 3 | Provider result returned unchanged | `middleware contract > provider result returned unchanged` | PASS | engine test kit |
| 4 | Provider exceptions propagate | `middleware contract > provider exceptions propagate` | PASS | engine test kit |
| 5 | Async-generator chunks forwarded (order, count, final value) | `middleware contract > async-generator chunks forwarded correctly (order, count, final value)`; `… early close is forwarded and recorded as abandoned` | PASS | engine test kit |
| 6 | Callback-entry context frozen | `provenance and identity > callback-entry context frozen` | PASS | engine test kit |
| 7 | Delayed result keeps original identity across a logical session change | `provenance and identity > delayed result uses original identity across a logical session change` | PASS | engine test kit |
| 8 | Plugin-raised lifecycle-shaped events cannot forge a native outcome | `provenance and identity > plugin-raised lifecycle-shaped events cannot forge a native outcome` | PASS | engine test kit + declaration |
| 9 | Engine dispatch origin alone does not prove downstream core execution | `provenance and identity > engine dispatch origin alone does not prove downstream core execution` | PASS | engine test kit |
| 10 | Accepted-human / core-acceptance proof for `prompt.submit` | kit: `provenance and identity > accepted-human core-acceptance proof for prompt.submit`, `… upstream middleware cannot rewrite prompt origin`; native: runs 1–3 | PASS | engine test kit + native session |
| 11a | Actor spawn provenance (`agent.spawn`) | kit: `provenance and identity > agent.spawn provenance requires a settled core trace`; native: run 3 | PASS | engine test kit + native session |
| 11b | `$.agent.list()` (and `$.session.id()`) interceptability | `provenance and identity > host reads $.agent.list() and $.session.id() are middleware-interceptable` | PASS (interceptable: lower tier) | engine test kit |
| 12 | Reload: `register` runs again | native run 2; kit: `mod delivery > module load runs register afresh with a new source epoch`, `… a second test has another source epoch and sequence restarts` | PASS | native session |
| 12k | Reload inside the kit | `mod delivery > module retirement/reload: the kit cannot hot-reload a module (config.set)` | NOT_SUPPORTED by the kit (assertion passes: no reload happens) | engine test kit |
| 13 | Logical session changes (`session.end` clear; `classic.SessionStart` clear/resume) | `provenance and identity > logical session changes (session.end clear, classic.SessionStart clear and resume)`; `… a normal session end runs one bounded drain; a clear does not` | PASS | engine test kit (+ native `startup`/`other` in runs 1–3) |
| 14 | Short-circuiting downstream middleware | `provenance and identity > short-circuiting downstream middleware is passed through unchanged` | PASS | engine test kit |
| 15 | Mod-batch receipts | `mod delivery > mod batch receipts: a valid receipt removes the records`; native runs 1 and 3 (every batch committed) | PASS | engine test kit + native session |
| 16 | Partial commit + timeout retry | `mod delivery > partial commit followed by timeout retries with the original UUIDs` | PASS | engine test kit |
| 17 | Malformed receipt is not acceptance | `mod delivery > malformed receipt is not acceptance` (11 cases) | PASS | engine test kit |
| 18 | Stable UUID replay | `mod delivery > stable UUID replay of unaccepted records`; native run 2 first epoch | PASS | engine test kit + native session |
| 19 | Queue bounds (2,048 records / 8 MiB) and batch bounds (128 / 64 KiB) | `mod delivery > queue bounds (2,048 records) and batch bounds (128 records, 64 KiB)`, `… batch record bound (128) binds for small records`, `… queue byte bound (8 MiB) with long escaped identifiers` | PASS | engine test kit |
| 20 | Retry backoff 250 ms → 5 s, reset on success; one pending timer; no idle timer; one in-flight drain | `mod delivery > retry backoff runs 250 ms to 5 s and resets on success`, `… one pending retry timer during backoff`, `… no timer while idle`, `… one in-flight drain per module`; native run 2 | PASS | engine test kit + native session |
| 21 | Module retirement/reload cancels timers | native run 2 (`native/run2-analysis.json`) | PASS | native session |
| 22 | No WorktreeCreate/WorktreeRemove handler; no worktree tool interception | `provenance and identity > no worktree handlers or worktree tool interception`; `validate-output.txt` hook list | PASS | engine test kit + validate |
| 23 | Observations sanitized | `middleware contract > observations carry no prompt text, tool input or output, or assistant text` | PASS | engine test kit |
| 24 | Module loads in a real session; native outcomes carry engine origin and a settled core trace | native runs 1–3 | PASS | native session |
| F1 | `acceptedInputProvenance` | decision below | **true** | declaration + engine test kit + native session |
| F2 | `automaticHumanFollowupResolution` | decision below | **false — NOT_SUPPORTED** | declaration |

`mutation-check.txt`: eleven deliberate defects (receipt cap removed, exit code ignored, no backoff doubling, `next` called twice, session read at result time, worktree tools not excluded, each of the four bounds raised tenfold, prompt text leaked) were each applied to a scratch copy; every one made at least one named test fail. No test is vacuous against those defects.

### Item notes

1–3. Each event's kit bottom counted one arrival per dispatch (`session.start`, `classic.SessionStart`, `session.attach`/`detach`, `prompt.submit`, `turn.start`, `turn.step`, `tool.check`, `tool.call`, `agent.spawn`, `turn.complete`, `session.end`). Inputs reached the bottom deep-equal to what the test raised (including `origin`, `turnId`, `wait`, `attachments`, `refusal`, `agentId`, `parentAgentId`, `isTeammate`, `name`); results came back deep-equal (including `ref`, `text`, `isReadOnly`, `context`, `usage`, `teammateId`, `reason`, `rule`).

4. A hook that throws is *skipped* by the engine (fail-open), so a throwing kit hook does not itself propagate. The rejection that reaches the observer is the kit bottom's own, `no implementation for turn.complete`; the caller received exactly that text, the bottom was reached once (no second `next`), and the observer recorded `entry` + `provider-error`.

5. Six chunks (`thinking`, two `text`, `tool`, `input`, `stop`) arrived in order and deep-equal; the generator's own `done` value equalled the bottom's result. An early `return()` reached the bottom's `finally` and the observer recorded `abandoned`. Kit property, not observer behaviour: the test-side `HookStream.result` reads `undefined` even with no plugin hooked on `turn.step` (control run in the scratch probe), so the final value is asserted on `done`. The kit also refuses an `engine` chunk authored by a link and a `turn.step` answer whose `{ turnId, index }` differ from the step's.

6–7. A `prompt.submit` whose bottom answered 1,000 ms late (mock clock) produced a result record whose entry fields (epoch, entry sequence, `dispatchOrigin`, session, generation, IDs) equal the entry record's, while a `tool.call` entered in between has a higher entry sequence and a lower one than the late result. A `tool.call` held across `session.end(clear)` + `classic.SessionStart(clear, S2)` kept `sessionId: S1` and its generation on the result; the call entered after the change carries `S2`.

8. An inline `user`-tier plugin tried every lifecycle raise through its `$`: `turn.start`, `turn.step`, `turn.complete`, `session.start` and `session.end` are each `… is not a function` (2.1.291 declares the plugin-facing `turn` noun as `{ abort }` alone and `session` without `start`/`end`). The observer recorded no lifecycle event from it. The plugin's `$.prompt.submit({ asUser: true, origin: { kind: 'composer' } })` and `$.tool.call` reached the observer with `dispatchOrigin: { plugin: "forger", tier: "user" }`, `engineDispatch: false` and prompt origin `{ kind: "plugin", name: "forger", asUser: true }` — the supplied `composer` origin was not honoured. (The host refuses a plugin prompt submitted from inside `command.run`; the test submits it from a timer.)

9, 14. An inline `append`-tier plugin beneath the observer answered `prompt.submit`, `tool.call`, `agent.spawn` and `turn.complete` without `next`. Each record has `engineDispatch: true` and `core: { links: 1, endPlugin: "shortcut", endTier: "append", endOutcome: "returned", coreSettled: false }`; the kit bottom was never reached. The answers came back unchanged, numbered once per dispatch.

10. Kit: a `composer` prompt with `turnId: "turn-active"` and `wait: true` is recorded as `{ origin: { kind: "composer" }, activeTurnIdAtSubmission: "turn-active", wait: true, attachmentCount: 0 }`; `bridge` is kept; a bottom `{ drop }` is recorded `dropped`; the only order fields on any record are the observer's own counters (`sequenceMeaning: "OBSERVER_CAPTURE"`). A `prepend`-tier plugin's `next({ ...e, origin: { kind: 'composer' } })` on a `scheduled-trigger` prompt did not change the origin the observer or the bottom saw. Native (runs 1–3): every `prompt.submit` has `dispatchOrigin` engine/core and `core: { links: 1, endPlugin: "engine", endTier: "core", endOutcome: "returned", coreSettled: true }`; origins observed were `sdk` (the `-p` prompt) and `task-notification` (run 3's background-agent completion re-entering as a prompt). A `composer` origin needs an interactive terminal session, which this qualification does not start; `composer`/`bridge` stamping rests on the declaration and the kit.

11a. Kit: a spawn answered by the kit bottom records `agentId: "agent-for-tu-spawn-real"`, `actorNativeId: "agent-parent"` (the `parentAgentId`); a spawn answered by the `append` shortcut records the plausible `agentId: "agent-fabricated-1"` with `coreSettled: false`, so the relation is not established. Native run 3: `tool.call(Agent)`, `tool.check(Agent) → allow` and `agent.spawn` share `tool_use_id toolu_0141ck3uiYjbCHpm5ZYbXhCf`; the spawn (`provider` engine/core, `background: true`) settled in core with `agentId: "a2cca502039b26b8c"`; that subagent's `turn.step` and `turn.complete` (reason `answer`) carry `actorNativeId: "a2cca502039b26b8c"` and their own `turnId`, and no `turn.start` (as declared).

11b. An `append`-tier plugin answered `agent.list` with `[{ id: "ghost-agent", … }]` and `session.id` with `"S-fabricated"`; a `user`-tier caller received both as bare values (keys `description, id, status, type`; no trace or provenance). The observer's own bootstrap `$.session.id()` read was intercepted the same way and is kept as `sessionIdSource: "session.id"`, superseded by the engine-stamped `classic.SessionStart` `session_id`. The production observer does not call `$.agent.list()`.

12, 21. Native run 2 (`native/run2-analysis.json`, seconds after process start): the stand-in helper failed every batch of the first source epoch. That epoch tried at 0.332, 0.649, 1.206, 2.269 and 4.326 (gaps 0.317 / 0.557 / 1.063 / 2.057 s: the 250/500/1000/2000 ms backoff plus helper start-up), leaving a 4 s retry pending, due at 8.326. The copy's `register.ts` was edited at 4.350; the engine reported `ui_log` "reloaded (12 hooks: …)" at 5.065 and the debug log `hooks module threadspace-observer@inline reloaded in 4.8ms`. A new source epoch started (`session.start` entry/result/bootstrap fired again; `classic.SessionStart` did not). The first epoch made **no** attempt after the reload through exit at 26.161, where a surviving timer would have tried at 8.326, 13.326, 18.326 and 23.326. Its 13 unacknowledged records were discarded with the environment (SPEC §8.3 states this loss). The kit cannot drive a reload: answering `config.set` re-ran nothing (same epoch, same argv), which `… the kit cannot hot-reload a module (config.set)` asserts.

13. Kit sequence: `startup S1` → `session.end(clear)` → (no session; `sessionIdSource: "session.end"`) → `clear S2` → `compact S2` (same generation) → `session.end(resume)` → `resume S3`; generations rise on every identity change only. A `prompt_input_exit` end delivered one batch with `timeoutMs: 100` before the hook returned; a `clear` end did not. Native runs ended with `reason: "other"` and the end drain delivered the `session.end` result (runs 1–3).

15–18. The batch envelope is `{ receiptVersion: 1, kind: "mod-batch", sourceEpoch, droppedRecords, records }`; argv is the configured `captureArgv`; `timeoutMs` is 250 (100 on the end drain). Statuses `COMMITTED`, `ALREADY_COMMITTED` and `LOCAL_SPOOLED` remove records; `NOT_ACCEPTED` records are resent byte-identical. Partial commit + timeout: the retry at 250 ms carried the same four UUIDs in the same order; the helper's journal held four records. Malformed receipts, each retried with the original UUIDs: no receipt with exit 0; not JSON; a JSON array; version 2; a missing result; an unknown UUID; a duplicate UUID; status `MAYBE`; over 16 KiB; exit 1 with a valid receipt; truncated output with a valid receipt. Natively (run 2), each failed first-epoch retry resent the earlier records first, same UUIDs, byte-identical.

19. 1,100 tool calls (2,200 records) with no drain: 2,048 delivered, the oldest 152 evicted, `droppedRecords: 152`, delivery in capture order. 1,000 spawns with 256-character identifiers of escaped control characters (~6 KiB records): fewer than 1,800 delivered, total bytes ≤ 8 MiB and within 16 KiB of it. 300 `session.attach` calls: batches of exactly 128 records with byte room left. Every batch ≤ 64 KiB.

20. Retry times with a failing helper: 0, 250, 750, 1,750, 3,750, 7,750, 12,750, 17,750, 22,750 ms; after a success the next failure restarts at 0, 250, 750. Thirty enqueues during a pending retry scheduled no timer and caused no early attempt. After delivery, ten idle minutes produced no timer and no batch. With the helper taking 100 ms, at most one run was in flight and starts were ≥ 100 ms apart.

22. `validate-output.txt` lists 12 hooks and no `classic.Worktree*`; `tool.call` and `tool.check` carry the matcher `{ tool: /^(?!(?:EnterWorktree|ExitWorktree)$)/ }`. In the kit, `classic.WorktreeCreate`, `classic.WorktreeRemove`, `tool.call(EnterWorktree/ExitWorktree)` and `tool.check(EnterWorktree)` produced no record, while a `Read` call produced two.

23. Sentinels placed in the prompt text, tool input, tool output, assistant answer, spawn task and streamed chunk text appear in none of the batches.

## Capability decisions

### `acceptedInputProvenance` = **true**

All three parts SPEC §7.3/§11.4 require are present for `prompt.submit` in 2.1.291:

1. **Engine dispatch origin.** `next.origin`, host-stamped (line 6366–6377): "Who raised this dispatch … the engine reads `{ plugin: "engine", tier: "core" }`." / "Set by the host alone, from the environment the call came from (its own MessagePort) and that plugin's seat; nothing a plugin writes reaches it." Kit: a plugin's prompt carries that plugin's origin. Native: engine/core on every prompt.
2. **Original composer/bridge origin.** `PromptSubmitInput.origin: PromptOrigin` (line 8782), "set by the engine where it was queued … `next(e)` passes it on as received; no hook may set one" (8776–8780); `kind: 'composer'` is "The user's own gesture at the terminal, as the engine stamped it (never presumed from an unstamped command)" (8556–8563); `kind: 'bridge'` (8569); a plugin's `asUser` prompt stays `{ kind: 'plugin', name, asUser: true }` "for every hook and provenance gate" (8650–8668). Kit: a `prepend` rewrite of origin did not take; a plugin's claimed `composer` origin became `plugin`.
3. **Settled core-acceptance proof.** `next.trace` (6378–6386): "What settled beneath this hook on its latest `next()` call … an entry per link beneath, nearest first, the engine's last … it ends short of the engine at a link that answered its last call itself"; a trace entry's plugin is `"engine"` "for the engine's own core or bottom" (12832); `PromptSubmitResult` "`next(e)` resolves once the prompt entered the session and its turn started, or it was queued behind the running one" (8789–8791), and `{ drop }` when it did not enter. The observer keeps only `{ links, endPlugin, endTier, endOutcome, coreSettled }` and `outcome`. Kit: a lower middleware's answer leaves `coreSettled: false`. Native: `coreSettled: true` on real prompts.

Limits of this true: `composer` was not exercised natively (no interactive session is started here); a hook *above* the observer (another `user` plugin or a managed `prepend` plugin) could still delay or hide a submission before the observer sees it, which SPEC §11.4 already treats as lowering the affected capability.

### `automaticHumanFollowupResolution` = **false** (facet NOT_SUPPORTED)

No positive original-order witness exists in 2.1.291:

- `PromptSubmitInput` (8740–8783) carries `text`, `attachments?`, `context?`, `turnId?`, `wait`, `origin` — no original-submission sequence, timestamp or predecessor relation. `turnId` is "The id of the model turn that was running when the prompt was submitted … Absent for a prompt submitted while the session was idle, and for a plugin's own" (8758–8766): it says which turn an input cannot resolve, and its absence is not a positive witness (SPEC §7.3).
- A search of the API module (lines 101–14433) for `submittedAt`, `queuedAt`, `enqueued`, `sequence`, `seq`, `timestamp`, `…At` time fields, `causal` and `predecessor` found none on prompt, turn or append events. The time fields that exist describe other things: `$.session.usage().startedAt` (session start), `version().builtAt`, a rate-limit `resetsAt`, a server tool's `startedAt`/`endedAt`, telemetry `loggedAt`; the `seq` fields belong to built-in tools' outputs (16501+).
- `session.append` (10235–10260) carries `message`, `door`, `origin`, `uuid`, `agentId`; its declaration says the row's time stays off the event: "Not on `e`, so stored as made: … the row's timestamps, parent links and provenance stamps" (10230–10231). Its order is the order rows enter the conversation, and a prompt typed mid-turn is queued first ("the message queue's own record of a prompt as it was queued … which is no row of the conversation", reference.md).
- `telemetry.log` collector records carry `loggedAt`, "an ISO 8601 instant to the millisecond" (12135–12136), but `event: string` and untyped `attributes` (12119–12131) declare no prompt-submission record or meaning, the time is wall time at logging (SPEC §5.4: wall time does not order), and the stream exists only where an operator collector is configured. Not a qualified witness; the observer hooks no telemetry stream.
- The observer's `callbackEntrySequence`/`callbackResultSequence` order its own callbacks only (`sequenceMeaning: "OBSERVER_CAPTURE"`); an upstream hook can delay a dispatch, so they are not original order (SPEC §5.4).

Explicit **Mark handled** remains the path for resolving a previous output after a follow-up; native outcomes, identity, actors and current-session Return are unaffected. Verifying the Mark handled control itself belongs to the attention router, which does not exist before M1.

## Build behaviour that contradicts SPEC §11.4 (for a decision record)

1. **Plugins cannot raise lifecycle events through `$`.** SPEC §11.4 states "Public `EventCalls` permits plugins to raise `$.turn.start/step/complete` and `$.session.start/end` on the same middleware chain." In 2.1.291 `EventCalls` (4608–4676) is "The engine's own events as calls on `$`" and is what a *test's* `$` carries (`Engine`, 14562–14574), but the plugin-facing `CoreEngineInterface` declares `turn: { abort }` alone (2847–2864) and a `session` noun without `start` or `end` (2670–2846). At run time a plugin's `$.turn.complete`, `$.turn.start`, `$.turn.step`, `$.session.start` and `$.session.end` are each `… is not a function` (kit, item 8). The forged-outcome path the SPEC guards against is closed in this build; the observer still records `next.origin` so a build that reopens it is caught. Plugin-*caused* lifecycle remains possible through other calls: `$.turn.abort` ends a running turn (the engine then raises `turn.complete`, presumably with engine origin — not exercised), and `$.agent.spawn` runs the Agent tool "under this call's origin" (3103–3105).
2. **After a hot reload the logical session is known only at the lower tier.** `classic.SessionStart` does not re-fire on reload (native run 2); the reloaded environment learns its session only from `$.session.id()`, which is middleware-interceptable (item 11b), until the next `classic.SessionStart` or `session.end`. SPEC §11.4 lists `$.session.id()` as bootstrap metadata only, so records between a reload and the next engine-stamped session event carry a lower-tier session key (`sessionIdSource: "session.id"`).
3. **Loading a mod folder writes into it.** At every load the engine writes `.claude-plugin/types/` (with its own `.gitignore`) and a root `tsconfig.json` into a `--plugin-dir` folder (`native/run1-debug-excerpt.log`). Loading `packages/provider-mod` in place would leave an untracked `tsconfig.json` in the repository; the native runs used a copy. This bears on SPEC §10 "installation and removal ownership" rather than §11.4.

Not contradictions, recorded for later units: `tool.call` carries no `turnId` (`ToolCallInput = ToolCallEnvelope & AgentLoop`, 12329), so a main-loop tool joins its turn through the active main turn and a subagent's through `agentId`; the engine's `turn.step` trace entry carries no `chunks` count natively (`chunksBeneath: null`); in `-p` the Agent tool ran in the background and its completion re-entered as a `task-notification` prompt; `turn.complete` reasons other than `answer`, and Stop-veto/continuation behaviour, were not exercised natively.

## Native session runs

| Run | Purpose | Outcome | Files |
| --- | --- | --- | --- |
| 1 | Module loads in a real session; lifecycle records | exit 0, `success`; 6 batches, 15 records, all committed; order `classic.SessionStart(startup)` → `session.start` → `prompt.submit(sdk)` → `turn.start` → `turn.step` → `turn.complete(answer)` → `session.end(other)`, each engine/core with `coreSettled: true` | `native/run1.json`, `run1-batches.jsonl`, `run1-debug-excerpt.log`, `run1-generated-types.json` |
| 2 | Hot reload, timer cancellation, native backoff and replay | exit 0, two `success` turns; 10 batches (5 failed first-epoch, 5 committed second-epoch), 56 record sends, 26 unique | `native/run2.json`, `run2-batches.jsonl`, `run2-analysis.json`, `run2-debug-excerpt.log`, `run2-generated-types.json` |
| 3 | Real subagent spawn | exit 0, `success`; 11 batches, 35 records, all committed | `native/run3.json`, `run3-batches.jsonl`, `run3-debug-excerpt.log`, `run3-generated-types.json` |

Debug excerpts keep only lines naming the plugin, `reload` or `--plugin-dir`. Each run also left an empty `~/.claude/projects/<run>-work/memory` folder despite `--no-session-persistence`, and run 3 a 171-byte `subagents/agent-a2cca502039b26b8c.meta.json` for its spawn; those were identified by name and content as this harness's and removed. The stand-in helper took 55–88 ms per run under Python; the production helper's budget (SPEC §8.2) is not measured here.

## Attempt log

Failed runs during development stay recorded here; none was an observer-behaviour failure on the final code.

1. Scratch probe: `claude plugin validate` refused a module that stored `$` ("`$` itself is assigned … `$` is always spelled `$.noun.event(...)` at the call site"). The mod hands delivery closures that spell `$.process.run(…)` / `$.clock.after(…)` inside each hook instead.
2. Scratch probe: inline plugins cannot read the test file's variables ("afters is not defined"); `mock.clock` owns the test's `clock.after` hook ("registered twice"); `session.start` does not fire on load in the kit; `config.set` has no kit implementation, and answering it does not reload.
3. Middleware suite, first run: 3 failures — the kit refused a test-authored `engine` chunk ("yielded a chunk with kind engine but a ref this link never pulled"). Second run: 4 failures — a `turn.step` answer must echo the step's `{ turnId, index }`; the provider-exception rejection is the kit bottom's `no implementation for turn.complete`; `HookStream.result` read `undefined` (a control run with no plugin hook on `turn.step` reads `undefined` too).
4. Provenance suite, first run: 1 failure — the host refused the forger's `$.prompt.submit` from `command.run` ("it would wait on the turn this hook is holding; submit from a later event").
5. Delivery suite: the first 8 MiB test queued only 7,700,588 bytes (900 tool calls, ~4.3 KiB records) and failed its own "within 16 KiB of 8 MiB" assertion; the fixture became 1,000 wide spawns. The first 128-record test failed `largest record < 512` (largest 542 bytes); the assertion became "byte room remained in each 128-record batch".
