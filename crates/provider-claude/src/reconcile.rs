//! Planning one reconciliation step for direct interactive Claude (SPEC §4.2,
//! §4.9, §4.14, §6.2). Stored live activations are compared with fresh kernel
//! samples and the pass's accepted joins:
//!
//! - kernel says the incarnation is gone (or the PID now has another birth):
//!   that activation ended — for an embedded CLI process exit ends its runtime;
//! - same PID and birth, different executable: the provider-process proof is
//!   void and the activation ends;
//! - the same process is now *joined* (bracketed, Section 4.4) to another
//!   session: the old activation was superseded in place (clear/resume/switch);
//! - anything weaker — a missing row, an unreadable process, a provisional
//!   candidate — proves nothing and changes nothing.
//!
//! Ends are addressed by execution ID, so an old end can never close a newer
//! activation of the same session (A→B→A).

use std::collections::HashMap;

use threadspace_surfaces_macos::process::{Incarnation, ProcessError};

use crate::discovery::{DiscoveryPass, Join};

/// A stored live activation and the process proof it was bound with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveExecution {
    pub execution_id: String,
    pub native_session_id: String,
    pub pid: i32,
    pub start_seconds: u64,
    pub start_microseconds: u32,
    /// Canonical executable identity recorded with the activation.
    pub executable: String,
    pub has_valid_binding: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// The kernel reports the incarnation gone (exit, or PID now reused).
    ProcessExited,
    /// Same PID and birth now runs a different executable.
    ExecutableChanged,
    /// The same process is now proven to host another session.
    SessionSwitched,
}

impl EndReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::ProcessExited => "PROCESS_EXITED",
            Self::ExecutableChanged => "EXECUTABLE_CHANGED",
            Self::SessionSwitched => "SESSION_SWITCHED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// (execution ID, reason) to end, with their bindings invalidated.
    pub ends: Vec<(String, EndReason)>,
    /// Live activations the pass reconfirmed: (execution ID, join index).
    pub continuing: Vec<(String, usize)>,
    /// Joins that start a new activation (join index).
    pub new_activations: Vec<usize>,
}

fn same_key(live: &LiveExecution, incarnation: &Incarnation) -> bool {
    incarnation.sample.pid == live.pid
        && incarnation.sample.start_seconds == live.start_seconds
        && incarnation.sample.start_microseconds == live.start_microseconds
}

/// `liveness` holds one fresh kernel sample per stored live execution ID.
pub fn plan(
    live: &[LiveExecution],
    liveness: &HashMap<String, Result<Incarnation, ProcessError>>,
    pass: &DiscoveryPass,
) -> Plan {
    let mut out = Plan::default();
    let mut ended: Vec<&str> = Vec::new();
    for execution in live {
        let Some(sample) = liveness.get(&execution.execution_id) else {
            continue;
        };
        let reason = match sample {
            Err(ProcessError::Vanished { .. }) => Some(EndReason::ProcessExited),
            // Denied/short/racing reads prove nothing about the process.
            Err(_) => None,
            Ok(now) if !same_key(execution, now) => Some(EndReason::ProcessExited),
            Ok(now) if now.executable.canonical() != execution.executable => {
                Some(EndReason::ExecutableChanged)
            }
            Ok(_) => match pass.join_for_pid(execution.pid) {
                Some(join)
                    if same_key(execution, &join.after)
                        && join.native_session_id != execution.native_session_id =>
                {
                    Some(EndReason::SessionSwitched)
                }
                _ => None,
            },
        };
        if let Some(reason) = reason {
            out.ends.push((execution.execution_id.clone(), reason));
            ended.push(&execution.execution_id);
        }
    }
    for (index, join) in pass.joins.iter().enumerate() {
        let current = live.iter().find(|execution| {
            !ended.contains(&execution.execution_id.as_str())
                && execution.native_session_id == join.native_session_id
                && same_key(execution, &join.after)
                && execution.executable == join.after.executable.canonical()
        });
        match current {
            Some(execution) => out.continuing.push((execution.execution_id.clone(), index)),
            None => out.new_activations.push(index),
        }
    }
    out
}

/// Joins that need a surface proof: new activations and continuing ones that
/// have no valid binding.
pub fn needs_surface<'a>(
    plan: &Plan,
    live: &[LiveExecution],
    pass: &'a DiscoveryPass,
) -> Vec<&'a Join> {
    let mut joins: Vec<&Join> = plan
        .new_activations
        .iter()
        .map(|index| &pass.joins[*index])
        .collect();
    for (execution_id, index) in &plan.continuing {
        let unbound = live.iter().any(|execution| {
            &execution.execution_id == execution_id && !execution.has_valid_binding
        });
        if unbound {
            joins.push(&pass.joins[*index]);
        }
    }
    joins
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::discovery::discover;
    use crate::discovery::tests::{Kernel, Script, VERSIONS, claude, row, snapshot};

    fn live(id: &str, session: &str, pid: i32, birth: u64) -> LiveExecution {
        LiveExecution {
            execution_id: id.into(),
            native_session_id: session.into(),
            pid,
            start_seconds: birth,
            start_microseconds: 7,
            executable: claude(pid, birth, Some(5)).executable.canonical(),
            has_valid_binding: true,
        }
    }

    fn pass_with(rows: Vec<crate::inventory::InventoryRow>, kernel: &Kernel) -> DiscoveryPass {
        let script = Script(RefCell::new(
            vec![Ok(snapshot(rows.clone())), Ok(snapshot(rows))].into(),
        ));
        discover(&script, kernel, |path| path.starts_with(VERSIONS)).expect("pass")
    }

    #[test]
    fn a_still_joined_activation_continues_without_a_new_activation() {
        let kernel = Kernel::new(vec![(11, vec![Ok(claude(11, 1000, Some(5)))])]);
        let pass = pass_with(vec![row(11, "A")], &kernel);
        let stored = vec![live("e1", "A", 11, 1000)];
        let liveness = HashMap::from([("e1".to_owned(), Ok(claude(11, 1000, Some(5))))]);
        let plan = plan(&stored, &liveness, &pass);
        assert!(plan.ends.is_empty());
        assert_eq!(plan.continuing, vec![("e1".to_owned(), 0)]);
        assert!(plan.new_activations.is_empty());
        assert!(needs_surface(&plan, &stored, &pass).is_empty());
    }

    #[test]
    fn process_exit_and_pid_reuse_end_the_old_activation() {
        let kernel = Kernel::new(vec![(11, vec![Ok(claude(11, 2000, Some(5)))])]);
        // PID 11 was reused by a new claude hosting session B.
        let pass = pass_with(vec![row(11, "B")], &kernel);
        let stored = vec![live("e1", "A", 11, 1000), live("e2", "C", 12, 1000)];
        let liveness = HashMap::from([
            ("e1".to_owned(), Ok(claude(11, 2000, Some(5)))),
            ("e2".to_owned(), Err(ProcessError::Vanished { pid: 12 })),
        ]);
        let plan = plan(&stored, &liveness, &pass);
        assert_eq!(
            plan.ends,
            vec![
                ("e1".to_owned(), EndReason::ProcessExited),
                ("e2".to_owned(), EndReason::ProcessExited)
            ]
        );
        assert_eq!(plan.new_activations, vec![0], "B gets its own activation");
    }

    #[test]
    fn an_in_place_switch_is_proven_only_by_a_bracketed_join() {
        // A→B in the same process: the join for PID 11 now names B.
        let kernel = Kernel::new(vec![(11, vec![Ok(claude(11, 1000, Some(5)))])]);
        let pass = pass_with(vec![row(11, "B")], &kernel);
        let stored = vec![live("a1", "A", 11, 1000)];
        let liveness = HashMap::from([("a1".to_owned(), Ok(claude(11, 1000, Some(5))))]);
        let switched = plan(&stored, &liveness, &pass);
        assert_eq!(
            switched.ends,
            vec![("a1".to_owned(), EndReason::SessionSwitched)]
        );
        assert_eq!(switched.new_activations, vec![0]);

        // B→A later: A's earlier activation was ended by ID, so A at PID 11
        // is a fresh activation; B's activation ends.
        let pass_back = pass_with(vec![row(11, "A")], &kernel);
        let stored_back = vec![live("b1", "B", 11, 1000)];
        let liveness_back = HashMap::from([("b1".to_owned(), Ok(claude(11, 1000, Some(5))))]);
        let back = plan(&stored_back, &liveness_back, &pass_back);
        assert_eq!(
            back.ends,
            vec![("b1".to_owned(), EndReason::SessionSwitched)]
        );
        assert_eq!(back.new_activations, vec![0]);
    }

    #[test]
    fn missing_rows_and_unreadable_processes_prove_nothing() {
        let kernel = Kernel::new(vec![]);
        let pass = pass_with(vec![], &kernel);
        let stored = vec![live("e1", "A", 11, 1000), live("e2", "B", 12, 1000)];
        let liveness = HashMap::from([
            ("e1".to_owned(), Ok(claude(11, 1000, Some(5)))),
            (
                "e2".to_owned(),
                Err(ProcessError::Denied { pid: 12, errno: 1 }),
            ),
        ]);
        let plan = plan(&stored, &liveness, &pass);
        assert_eq!(plan, Plan::default(), "absence from a list is not an end");
    }

    #[test]
    fn executable_replacement_ends_and_requalifies() {
        let mut replaced = claude(11, 1000, Some(5));
        replaced.executable.file_id = Some("16777234:99".into());
        let kernel = Kernel::new(vec![(11, vec![Ok(replaced.clone())])]);
        let pass = pass_with(vec![row(11, "A")], &kernel);
        let stored = vec![live("e1", "A", 11, 1000)];
        let liveness = HashMap::from([("e1".to_owned(), Ok(replaced))]);
        let plan = plan(&stored, &liveness, &pass);
        assert_eq!(
            plan.ends,
            vec![("e1".to_owned(), EndReason::ExecutableChanged)]
        );
        assert_eq!(
            plan.new_activations,
            vec![0],
            "requalified as a new activation"
        );
    }

    #[test]
    fn an_unbound_continuing_activation_needs_a_surface() {
        let kernel = Kernel::new(vec![(11, vec![Ok(claude(11, 1000, Some(5)))])]);
        let pass = pass_with(vec![row(11, "A")], &kernel);
        let mut stored = vec![live("e1", "A", 11, 1000)];
        stored[0].has_valid_binding = false;
        let liveness = HashMap::from([("e1".to_owned(), Ok(claude(11, 1000, Some(5))))]);
        let plan = plan(&stored, &liveness, &pass);
        assert_eq!(needs_surface(&plan, &stored, &pass).len(), 1);
    }
}
