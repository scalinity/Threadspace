# M0B synthetic safety tests

**Synthetic evidence never closes the native GO gate.** These tests drive the same adapter logic the companion runs natively, with scripted processes, inventories, Terminal enumerations and races. The tests that touch the real kernel (`process`, `tty`, `ancestry`) are labelled *native mechanism*: they read real macOS process metadata but involve no Claude or Terminal session.

`test-run.txt` holds the latest run on the candidate commit: 81 tests, 0 failed.

Every route refusal goes through the `refused()` helper in `crates/surfaces/src/tests.rs`. It asserts that no focus event was sent and that the result is not exact, so **no synthetic scenario can focus an unrelated surface without failing.**

| Required scenario | Tests |
| --- | --- |
| Same PID reused with a different birth | `surfaces::same_pid_with_a_different_birth_cannot_use_the_old_binding`; `discovery::same_pid_with_a_different_birth_across_the_bracket_is_rejected`; `discovery::pid_reused_before_the_first_sample_cannot_inherit_the_old_row`; `reconcile::process_exit_and_pid_reuse_end_the_old_activation`; journal `same_pid_with_a_new_birth_cannot_inherit_the_old_binding` |
| TTY path reused by another generation | `surfaces::a_reused_tty_path_cannot_revive_a_binding_whose_process_ended`; journal `a_reused_tty_path_does_not_resurrect_an_invalidated_binding` (natively observed in `native/F-…`) |
| Executable replacement | `surfaces::executable_replacement_is_refused`; `discovery::executable_replacement_within_the_same_pid_and_birth_is_rejected`; `reconcile::executable_replacement_ends_and_requalifies`; native mechanism `process::exec_keeps_pid_and_birth_but_changes_the_executable` |
| Stale binding revision | `surfaces::a_binding_revised_before_focus_is_refused` |
| Stale Terminal application generation | `surfaces::a_stale_terminal_generation_is_refused` (restart, and a change during enumeration) |
| Multiple eligible surfaces | `surfaces::several_eligible_tabs_are_ambiguous` |
| Provider current-session conflict | `surfaces::a_provider_session_change_is_a_conflict_not_a_route`; `surfaces::a_session_change_seen_after_focus_downgrades_verification`; `surfaces::a_missing_or_failed_provider_lookup_never_becomes_current` |
| Old queued route after the binding revision changes | `surfaces::an_old_queued_request_never_moves_focus` (stale expected revision; request past its two-second budget) |
| A→B→A with delayed evidence | `reconcile::an_in_place_switch_is_proven_only_by_a_bracketed_join`; journal `a_delayed_old_end_closes_only_its_own_activation_across_a_to_b_to_a` |
| Delayed old activation end | journal `a_delayed_old_end_closes_only_its_own_activation_across_a_to_b_to_a` |
| Multiple attachments | `surfaces::multiple_attachments_require_an_explicit_choice`; `discovery::one_session_in_two_processes_is_two_joins`; journal `one_session_with_two_live_attachments_keeps_both_bindings` |
| Readback disagreement | `surfaces::readback_must_name_the_target_device`; `surfaces::a_target_window_terminal_does_not_call_frontmost_is_not_exact`; `surfaces::a_frontmost_process_other_than_the_terminal_incarnation_is_not_exact`; `surfaces::terminal_not_frontmost_after_activation_is_not_exact` |
| Unsupported / unbound / unknown | `surfaces::unbound_fixture_and_unknown_sessions_open_the_inspector_only`; `discovery::background_missing_duplicate_and_new_rows_never_join` |
| Ancestry edges | `ancestry::*`: reparenting, reused parent, mid-edge replacement, vanish, cycle, depth limit, and the native walk stopping honestly at root-owned `login` |
| Racing Terminal enumeration | `terminal::rejects_incomplete_or_racing_enumerations` |
