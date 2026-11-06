# D-0012 — Claude Code 2.1.318 observer requalification

**Status:** PROPOSED — native qualification and independent review pending.
**Affects:** D-0005 and D-0009 (extends their contract to another build);
`crates/provider-claude/src/profiles.rs`;
`docs/compatibility/claude-observer-2.1.318.json`; MILESTONES M2 prerequisites.

## Context

The qualified observer builds were 2.1.291 (D-0005) and 2.1.295 (D-0009). Neither
executable is installed any longer. The installed launcher resolves to
`~/.local/share/claude/versions/2.1.318`
(SHA-256 `677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d`). Every
build outside the qualified list runs at the lower observer tier, which records
inputs but no native lifecycle outcomes, so M2's vertical slice cannot run on it.

## Evidence

[`evidence/M2/provider/claude-2.1.318/`](../../evidence/M2/provider/claude-2.1.318/README.md):

1. **Declarations.** All 97 declarations the observer depends on are present in
   the 2.1.318 authoring declarations and identical, comments stripped, to the
   retained 2.1.310 declarations. Against 2.1.295, the two declarations whose full
   text was retained (`ToolCallInput`, `MockSession`) match; the other 95 are
   NOT_VERIFIABLE because the 2.1.295 declaration bytes were not retained.
2. **Engine test kit.** The kit now raises one nested `tool.check` permission
   preflight inside each `tool.call`, and an `ask` decision denies before a
   mocked tool bottom. Nine fixtures that assumed one record pair per call and a
   permissive ask were corrected to pin exact occurrence IDs and event selections;
   three kit-behaviour controls distinguish nested dispatch from a duplicate load.
   `claude plugin validate` passes; `claude plugin test` passes 41/41. No observer
   source changed. Mutation controls still reject duplicated observations, a
   changed provider result, late Session context and fabricated core settlement.
3. **Portable suites.** Ownership 14/14 and latency 23/23 on the unchanged module.
4. **Loading.** Noninteractive loading admits the observer once per load. Type
   files are generated only with `CLAUDE_CODE_PLUGIN_DIR_WATCH=1` in that path;
   interactive loading is part of the native qualification below.
5. **Inventory and hooks.** The live `claude agents --json --all` row shape
   (interactive rows: `cwd`, `kind`, `name`, `pid`, `sessionId`, `startedAt`,
   `status`, `waitingFor`) matches the parser; background rows were not observed.
   All 15 setup hook events and the settings shape are accepted by 2.1.318's schema.

## Decision

- **2.1.318 is a candidate observer build with D-0005's capability profile
  unchanged:** `acceptedInputProvenance = true`,
  `automaticHumanFollowupResolution = false` (NOT_SUPPORTED). Explicit Mark handled
  remains the resolution path. `profiles.rs` admits it so that native
  qualification can exercise the qualified observer path.
- It becomes a qualified build only when the native interactive evidence below
  passes on a build that admits it and an independent review accepts this record.
  If that evidence fails, the admission is withdrawn.

## Native qualification required

Ordinary interactive sessions in harness-owned Ghostty panes: native turn
identity and completed outcomes across ten prompt/completion/follow-up cycles;
ownership and reload authority with an unchanged native session ID; Stop
continuation, interrupt, failure, blocked submission and incompatible-profile
cases; mod-batch delivery and receipts; parent-completed/child-waiting inventory.
Each is recorded with the exact provider executable hash and the Threadspace build.
