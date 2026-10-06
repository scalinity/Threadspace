# Threadspace patch of libsqlite3-sys 0.38.2

This directory is crates.io `libsqlite3-sys` 0.38.2, wired in through
`[patch.crates-io]` in the workspace `Cargo.toml`. It exists only so the
companion links the frozen SQLite engine **3.53.4**; see
`docs/decisions/D-0001-sqlite-3.53.4-binding.md`.

## Changes from the published crate

| Path | Change |
| --- | --- |
| `sqlite3/sqlite3.c`, `sqlite3/sqlite3.h`, `sqlite3/sqlite3ext.h` | Replaced with the official SQLite 3.53.4 amalgamation. |
| `sqlite3/bindgen_bundled_version.rs`, `sqlite3/bindgen_bundled_version_ext.rs` | `SQLITE_VERSION`, `SQLITE_VERSION_NUMBER`, `SQLITE_SOURCE_ID`, `SQLITE_SCM_TAGS`, `SQLITE_SCM_DATETIME` set to the 3.53.4 values. |
| `sqlcipher/` | Omitted. Only the `bundled-sqlcipher` features read it and Threadspace enables none of them. |
| `.cargo_vcs_info.json`, `Cargo.toml.orig` | Omitted packaging metadata. |

Nothing else differs. The 306 `SQLITE_API` function declarations of 3.53.2
and 3.53.4 are identical, and all 351 integer constants in the 3.53.4 header
equal the pregenerated binding constants.

## Source verification

| Item | Value |
| --- | --- |
| Download | `https://sqlite.org/2026/sqlite-amalgamation-3530400.zip` |
| Zip SHA3-256 (published on sqlite.org/download.html) | `628a44cfe82c66aed1ccbbe85a562d2e33ebe64b3288981ed76285612227934e` |
| Zip SHA-256 | `1e71ddf93849c6a6ecf58b827c0692073d2dd7ee40196158068f7b29f422e87d` |
| `sqlite3.c` SHA3-256 (published in the 3.53.4 release log) | `67f423e9ebbbdc473cbc4772c872ee6b89f31fde4ed0279a5c25d5f65c043a16` |
| `SQLITE_SOURCE_ID` | `2026-07-24 19:02:57 bf7c7f30031888f4e796e429ab3978879485813aaca6f641c7b33e4e09459bcc` |

The companion verifies `sqlite_version()` and `sqlite_source_id()` at startup
and refuses to open the journal when they differ from these values.
