# Failed attempt: re-admission check used a fresh capture time

Every "lost" record here is a harness defect, not a lost record. The
presence proof re-admits each acknowledged UUID and expects
ALREADY_COMMITTED at the original cursor. The journal's idempotency rule
treats a retry as identical only if its content matches, including the
capture time (`crates/journal/src/admission.rs` compares
`captured_wall_ms`), and the qualification handler stamped a new capture
time on each admission, so every re-admission answered CONFLICT. A real
capture keeps its capture time across retries; the qualification request now
carries it. `crash-rounds.jsonl.gz` is the raw record, compressed.

Valid observations from this run: second writer refused (exit 75, incumbent
unchanged); owner-command retry across a companion crash COMMITTED then
ALREADY_COMMITTED at cursor 58299; engine 3.53.4 / WAL / synchronous=2.
