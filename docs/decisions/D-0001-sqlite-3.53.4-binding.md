# D-0001 — Link SQLite 3.53.4 through a patched `libsqlite3-sys`

**Status:** Accepted; ratified during the independent M0C review on 2026-10-06 using inherited M0A/M0B and M0C engine/durability evidence. This does not accept M0C as a whole.
**Affects:** SPEC §9.1, §18.1 (SQLite engine row; "M0 pins the Rust binding/build inputs so this exact native engine is used"); MILESTONES M0A implementation requirements (SQLite `3.53.4` row) and G10.

## Context

The frozen candidate is SQLite **3.53.4**, linked into the native companion, with `sqlite_version()` and `sqlite_source_id()` verified at runtime. SPEC §18.1 states that relying on "a Rust crate name is insufficient".

At M0A kickoff (2026-10-06) the newest published `libsqlite3-sys` is **0.38.2**, and its bundled amalgamation is **3.53.2** (`SQLITE_SOURCE_ID 2026-06-03 19:12:13 d6e03d8c…`). No published release of the binding bundles 3.53.4, and its bundled build compiles `{CARGO_MANIFEST_DIR}/sqlite3/sqlite3.c` with no source override. Linking the system SQLite would give an unknown engine version.

The frozen candidate is therefore obtainable, but not through any published binding as-is.

## Decision

Use `rusqlite` 0.40.2 with its `bundled` feature, and patch its `libsqlite3-sys` 0.38.2 dependency through `[patch.crates-io]` with `third_party/libsqlite3-sys/`. That copy is identical to the published crate except:

- The official 3.53.4 amalgamation replaces the bundled 3.53.2 sources.
- The five version string/number constants in the pregenerated bindings carry the 3.53.4 values.
- The unused `sqlcipher/` tree is omitted.

`third_party/libsqlite3-sys/THREADSPACE-PATCH.md` records the source URL and verified hashes: zip SHA3-256 `628a44cf…934e` and `sqlite3.c` SHA3-256 `67f423e9…3a16`, both equal to sqlite.org's published values. The 3.53.2→3.53.4 C API is unchanged: 306 identical `SQLITE_API` declarations, and all 351 integer constants equal the pregenerated bindings.

The journal refuses to open unless the linked engine reports exactly `3.53.4` and source ID `2026-07-24 19:02:57 bf7c7f30…bcc`.

## Consequences

- The exact frozen engine is linked and runtime-verified; no architecture or version change.
- Replacing the binding later (for example, once a published `libsqlite3-sys` bundles 3.53.4 or a newer engine is deliberately selected) is a dependency update that repeats the G10 regression.
- `docs/compatibility/platform-lock.json` records the binding, the patch and the hashes.
