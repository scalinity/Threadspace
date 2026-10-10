# Superseded migration completeness — before journal-header boundary fix

This passing campaign predates the discovered zero-fact observation boundary counterexample. Its 288 core tests, 21 retained regressions and 24,400 permutations did not exercise a metadata-only tail after an upgrade. Source hashes and original outputs are preserved unchanged.

The later metadata-tail negative control showed why using only the first current-version fact could move an already committed transition. A separate mixed-checkpoint control showed the same boundary must be applied when recovering an older valid checkpoint. The final campaign in the parent directory includes durable JournalEntry admission versions, zero-fact tails, and mixed suffixes with post-upgrade owner and notification facts. This earlier campaign is not the final source-qualified migration result.
