# Capture-helper cold start: cause (diagnostic)

**Diagnostic only. This is not an F4 or capture-budget acceptance record, and it
changes no measured population, threshold or helper.** It answers the question the
[compact-helper comparison](../compact-helper/README.md) left open: what makes a
first helper execution slow.

`cold.py` copies an executable to new paths (same bytes and signature, new inodes)
and times three sequential executions of each copy from the parent, launch through
exit. Raw rows are in [`runs.json`](runs.json) (macOS build recorded there).

| Executable | First execution of each new inode | Repeat executions |
| --- | --- | --- |
| Installed Dev helper, first 6 copies after installation | 59.1–332.0 ms (83.1, **332.0**, **216.3**, 67.4, 65.3, 59.1) | 2.4–4.7 ms |
| Same helper, 30 later copies | p50 79.0, p95 88.1, max 88.3 ms; none over 250 ms | p50 3.5, max 4.3 ms |
| Ad-hoc-signed release helper, 6 copies | 54.3–74.1 ms | 2.9–4.1 ms |
| Trivial C program, 6 copies | 33.1–62.5 ms | 2.3–3.5 ms |

## Findings

1. The helper's own work is about 3 ms. The slow part precedes `main`.
2. macOS charges every new executable inode a first-execution cost: about 33 ms for a
   trivial binary and about 80 ms for the 1.7 MB helper, independent of whether it is
   ad-hoc or Developer-signed.
3. The first executions of **new executable content** (the first copies after a new
   helper build was installed) cost up to 332 ms here; byte-identical copies made
   afterwards stayed under 89 ms. This is consistent with the OS's one-time policy
   and malware assessment of new executable content (both files carry
   `com.apple.provenance`), and with the retained 618.911 ms first execution of a
   freshly built baseline in the compact-helper comparison.
4. No application design bounds time spent before its first instruction. A smaller
   binary lowers the size component; it cannot remove the content assessment.

## Consequence for the gate

The original `<250 ms` launch-through-reap control measures the first execution of a
freshly built helper, so it can fail on the OS assessment alone. The only mechanisms
that keep a capture from paying that assessment are executing a newly staged helper
once at setup, or defining the gate over executions of the installed, already
assessed helper. The current completion contract forbids both (no prewarming; no
change to the measured population). The capture-helper cold-budget gate therefore
stays **open, pending an owner decision on its requirement**; nothing here relabels
any retained failure.
