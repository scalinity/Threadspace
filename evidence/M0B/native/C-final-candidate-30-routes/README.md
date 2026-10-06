# M0B: final-candidate route regression (build `fc65641`)

This regression checks the stricter readback rule from failure ledger B-07, which requires Terminal's own frontmost-window flag, on the exact build submitted for review.

The run used two test sessions in the same cwd, because two of the original three were closed on purpose in tests F and H. Each was routed 15 times, round-robin: `5c4f593b…`/0d750db7 on ttys004 and `ea8e75e4…`/f8cd39d3 on ttys007.

| Metric | Result |
| --- | --- |
| Routes | **30** |
| Exact routes (exact surface, current session, foreground input) | **30/30** |
| Wrong targets | **0** |
| Unrelated binding changes | **0** |
| Terminal frontmost-window flag | true on 30/30 |
| Independent readback agrees | 30/30 |
| Latency | median 889 ms, p95 941 ms, max 943 ms |

The official three-session × 10 run is `../C-same-cwd-30-routes/` (build `8980d34`). Between `8980d34` and this build, only the readback acceptance rule changed, and the change can only turn an exact result into a refusal.
