# M1 evidence at the third remediation candidate (`579df5b`)

The fourth independent review examined candidate `579df5b7902396f706095a28b455f9189513daa5`. Its fourth remediation changed one evidence file, kept here byte for byte:

- **`manifest.json`:** the candidate's manifest.

Every other area of `579df5b` is unchanged in `evidence/M1/`. Fixtures, contracts, replay and crash reproduced their files byte for byte on the fourth remediation's source. Migration reproduced every field except the per-store file SHA-256 values, so its committed summary is kept. The checkpoint-tail evidence in [`../../remediation-3/`](../../remediation-3/README.md) also reproduced byte for byte. See [`../../remediation-4/README.md`](../../remediation-4/README.md).
