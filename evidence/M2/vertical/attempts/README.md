# Superseded vertical-slice runs

Every run here was superseded by `../20261009T041131Z-dev/`. Each one either exposed a harness or environment defect that was fixed before the next, or ran before a later evidence step existed. None is cited for acceptance. They are kept because they are the record of what the final harness corrects.

| Run | Outcome | What it showed | Corrected by |
| --- | --- | --- | --- |
| `20261009T023103Z-dev` | No summary | The companion never bound the session to its tab: the dev identity had no Terminal automation consent (status -1744, `AUTOMATION_NOT_AUTHORIZED`). The owner granted it once, a TCC dialog that software may not answer. | `activate` reports a missing consent as BLOCKED before any window opens |
| `20261009T025728Z-dev` | FAIL | Turns never completed: `do script` ends its text with a line feed, which Claude's prompt editor inserts as a newline, not a submit | Prompts end with a separate carriage return (`Tab::submit_line`) |
| `20261009T031538Z-dev` | FAIL | The first prompt was typed before the editor was ready (inventory and binding follow the process), so it went out with the next return as one turn | A start waits for Claude's ready prompt on the tab's screen |
| `20261009T032456Z-dev` | FAIL | The cycle ran, but the runner read the UI's answers outside their `result` envelope (no route, no rows) | UI answers read from `result` |
| `20261009T032843Z-dev` | No summary | The window failed to open: Terminal refused every element query (-1708) | Every Terminal script waits out a refusal and records it |
| `20261009T033254Z-dev` | FAIL | Return exact; `m2-fleet` never reported: a full-fleet report exceeds the 64 KiB report bound | `m2-fleet` takes a session filter |
| `20261009T033616Z-dev` | FAIL | As above, before the rebuilt UI was installed | The rebuilt UI |
| `20261009T034710Z-dev` | PASS (1 cycle) | First passing cycle; the first prompt still needed the 20 s return retry | — |
| `20261009T034825Z-dev` | PASS (1 cycle) | A 1.5 s return gap still fell inside the editor's paste window | A 3.5 s gap and a confirmed submit that resends the return every 4 s |
| `20261009T035405Z-dev` | PASS (10 cycles) | Ten cycles before Mark handled and latency were part of the run | Mark handled and latency steps |
| `20261009T040231Z-dev` | PASS (1 cycle) | First run with Mark handled and latency | — |
| `20261009T040337Z-dev` | PASS (10 cycles) | A real-time screen recording made Terminal refuse every element query; see its `ATTEMPT-NOTE.md` | The recording became a one-second still time-lapse |
