# M1 evidence at the second remediation candidate (`85d188e`)

The third independent review examined candidate `85d188e221c24a4e91a533d0ab743fa0e1184c80`. Its third remediation changed two evidence files, kept here byte for byte:

- **`manifest.json`:** the candidate's manifest.
- **`migration/summary.json`:** the migration run the second remediation's record cites, for example the WAL-without-`-shm` store's WAL `46aa8140…` in [`../../remediation-2/README.md`](../../remediation-2/README.md).

The migration area was rerun on the third remediation's source. Every field reproduced except the per-store file SHA-256 values. Each run creates fresh stores with new identifiers, so a store's WAL bytes differ between runs. Its main file's bytes do not, so a main-file hash such as `ae803b40…` appears in both runs.

Every other area of `85d188e` is unchanged in `evidence/M1/` (fixtures, contracts, replay and crash reproduced their files byte for byte), or is kept in [`../f7e9a6c/`](../f7e9a6c/README.md) and the current directories as before. See [`../../remediation-3/README.md`](../../remediation-3/README.md).
