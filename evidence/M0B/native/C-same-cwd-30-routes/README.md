# M0B native test C: three same-cwd sessions, ten routes each

**Result: PASS.** Run on the build from `8980d34`, which has the kernel-sourced Terminal incarnation. A re-run on the final candidate build is in `../C-final-candidate-30-routes/`.

| | value |
| --- | --- |
| Sessions (one cwd) | `a52de5c2…`/efa96221 (ttys002), `5c4f593b…`/0d750db7 (ttys004), `ea8e75e4…`/f8cd39d3 (ttys005) |
| Routes | 3 × 10 = **30**, round-robin, from `threadspace-qualify prod route-loop` |
| Exact + current + foreground | **30/30** |
| Wrong targets | **0** |
| Other tracked bindings changed | **0** (set comparison after every route) |
| Latency (companion-measured) | median 841 ms, p95 876 ms, max 901 ms; 2 s hard budget never exceeded |

Each record in `routes.jsonl` carries the full route evidence plus the harness's own checks, made after the route and independent of the companion:

- its own `claude agents` lookup for the expected session and a kernel sample of that process;
- its own Terminal readback of the selected tab, plus `stat(st_rdev)` of it;
- `lsappinfo` for the frontmost application.

All 30 records pass every check: the independently selected tab is the expected session's controlling device, it equals the companion's readback, Terminal is frontmost, exactly one `st_rdev` match, binding revision steady through focus, provider pid→session before and after focus, and the Apple-event sender is a companion child.

Superseded runs are kept under `../../attempts/`:

- `C-run1-launchservices-terminal-pid`: 25/30. Five routes refused themselves (`TERMINAL_GENERATION_CHANGED`, no focus), because NSRunningApplication lists transient osascript processes under Terminal's bundle ID. A reproducer is included. Fixed in `8980d34`.
- `C-run2-stale-harness-comparison`: 30/30 exact and 0 wrong. It ran on a stale harness binary whose ordered comparison over-reported binding changes.

The latency target (SPEC §20.2, p95 ≤ 750 ms) is **not met**: p95 is about 876 ms. The time is dominated by two `osascript` launches (~240 ms each) and two provider lookups (~175 ms each). It is recorded for M13; correctness was not traded for it.
