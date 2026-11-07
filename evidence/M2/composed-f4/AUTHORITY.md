# Composed F4 — authority record

**November 6, 2026, about 8:27 PM EST.** The owner authorized exactly one execution of the unchanged duration
conformance check (`tests/native/tools/m2_duration_conformance.py`, policy
`evidence/M2/remediation-7/instrument-policy.json`) on Claude Code 2.1.318
(`~/.local/share/claude/versions/2.1.318`, SHA-256
`677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d`), on an unloaded machine.

- This is a first qualification of a different provider runtime. The retained remediation-7 failure bound provider
  `c9b5341637becbd423ddffc5b254afb645682a3868cb708bbc6cc0e7bb419937` and stays FAILED and authoritative for that
  runtime.
- Limits are unchanged: each enclosure width at most 60 ms, observer allowance at most 30 ms, read step at most 1 ms,
  engineering scope 0..400 ms. Nothing is widened, calibrated or tuned.
- The check runs once. A failure is retained and F4 stays blocked; it is not repeated.
- Only a passing check permits building the distinct schema 3 composed exporter and, after its own controls pass,
  the frozen measurement cohort.

## Outcome

Executed once at 10:16 PM EST on November 6, 2026 (load average 4.82 / 7.39 / 10.06), report
[duration-conformance-2.1.318.json](duration-conformance-2.1.318.json): **qualified**. Enclosure widths 57.293,
56.830 and 57.027 ms (limit 60), observer allowance 29.554 ms (limit 30), native allowance 0.001 ms, largest read
step 0.014 ms (limit 1). Provider `677a2ea7…`, observer adapter `d461d6d6…`, policy `67cd285d…` (byte-identical to
remediation-7's), one boot, no power discontinuity. Raw provider output stays private outside Git. Two earlier
invocations stopped before any provider launch (an isolated interpreter could not import the runner's sibling
module, and a superseded wait was cancelled before it ran); neither froze a policy or started the provider. The
margins are narrow (about 3 ms of enclosure and 0.45 ms of allowance), so the qualification holds only for this
provider, adapter and policy, as the policy states.
