//! Deterministic discovery path A (SPEC §4.4). The first inventory response
//! only enumerates candidates. Each candidate PID is sampled (incumbent
//! sample), a second bounded inventory request is made, and the PID is
//! sampled again. A Session→process join is accepted only when the second
//! response still maps the same PID to the same full session ID as a direct
//! interactive client, and the process kept its kernel birth, executable and
//! controlling device across the bracket. Everything else stays provisional
//! with a reason; nothing loops waiting for a globally stable snapshot, and
//! nothing falls back to cwd, name, recency or terminal titles.

use std::collections::{BTreeMap, BTreeSet};

use threadspace_surfaces_macos::ancestry::Sampler;
use threadspace_surfaces_macos::process::{Incarnation, ProcessError};

use crate::inventory::{Inventory, InventoryError, InventoryRow, InventorySnapshot};

/// An accepted join: this full native session ID was the provider's mapping
/// for this process incarnation across the bracket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Join {
    pub native_session_id: String,
    pub pid: i32,
    /// Incumbent sample taken before the confirming lookup.
    pub before: Incarnation,
    /// Sample taken after the confirming lookup.
    pub after: Incarnation,
    /// Controlling device (`e_tdev`), identical in both samples.
    pub device: u32,
}

/// An inventory row that did not earn a join, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provisional {
    pub pid: Option<i64>,
    pub session_id: Option<String>,
    pub kind: Option<String>,
    pub reason: &'static str,
}

/// A full native session identity reported by the inventory, with the
/// provider-reported activity of its row. Identity facts do not need a
/// process join; routing does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedSession {
    pub native_session_id: String,
    pub kind: Option<String>,
    pub status: Option<String>,
    pub waiting_for: Option<String>,
    /// Provider display label; never a key.
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryPass {
    pub first: InventorySnapshot,
    pub second: Option<InventorySnapshot>,
    pub second_error: Option<String>,
    pub joins: Vec<Join>,
    pub provisional: Vec<Provisional>,
    pub sessions: Vec<ObservedSession>,
}

impl DiscoveryPass {
    pub fn join_for_pid(&self, pid: i32) -> Option<&Join> {
        self.joins.iter().find(|join| join.pid == pid)
    }
}

fn provisional(row: &InventoryRow, reason: &'static str) -> Provisional {
    Provisional {
        pid: row.pid,
        session_id: row.session_id.clone(),
        kind: row.kind.clone(),
        reason,
    }
}

fn sample_code(result: &Result<Incarnation, ProcessError>) -> Option<&'static str> {
    result.as_ref().err().map(ProcessError::code)
}

/// The rows of one snapshot that can be joined, keyed by PID. A PID that two
/// rows claim with different sessions is a provider conflict, not a choice.
fn candidates(
    snapshot: &InventorySnapshot,
    provisional_out: &mut Vec<Provisional>,
) -> BTreeMap<i32, (String, InventoryRow)> {
    let mut by_pid: BTreeMap<i32, Vec<&InventoryRow>> = BTreeMap::new();
    for row in &snapshot.rows {
        if !row.is_interactive() {
            provisional_out.push(provisional(row, "UNSUPPORTED_KIND"));
            continue;
        }
        match (row.live_pid(), row.full_session_id()) {
            (Some(pid), Some(_)) => by_pid.entry(pid).or_default().push(row),
            (None, _) => provisional_out.push(provisional(row, "MISSING_PID")),
            (Some(_), None) => provisional_out.push(provisional(row, "MISSING_FULL_SESSION_ID")),
        }
    }
    let mut out = BTreeMap::new();
    for (pid, rows) in by_pid {
        let ids: BTreeSet<&str> = rows
            .iter()
            .filter_map(|row| row.full_session_id())
            .collect();
        if ids.len() == 1 {
            let row = rows[0].clone();
            let id = ids.into_iter().next().unwrap_or_default().to_owned();
            out.insert(pid, (id, row));
        } else {
            for row in rows {
                provisional_out.push(provisional(row, "DUPLICATE_PID"));
            }
        }
    }
    out
}

/// Runs one discovery pass. Only a failed first lookup is an error; a failed
/// second lookup leaves every candidate provisional.
pub fn discover(
    inventory: &impl Inventory,
    kernel: &impl Sampler,
    qualifies: impl Fn(&str) -> bool,
) -> Result<DiscoveryPass, InventoryError> {
    let first = inventory.fetch()?;
    let mut provisional_rows = Vec::new();
    let first_candidates = candidates(&first, &mut provisional_rows);

    let before: BTreeMap<i32, Result<Incarnation, ProcessError>> = first_candidates
        .keys()
        .map(|pid| (*pid, kernel.incarnation(*pid)))
        .collect();
    let second = inventory.fetch();
    let after: BTreeMap<i32, Result<Incarnation, ProcessError>> = first_candidates
        .keys()
        .map(|pid| (*pid, kernel.incarnation(*pid)))
        .collect();

    let (second, second_error) = match second {
        Ok(snapshot) => (Some(snapshot), None),
        Err(error) => (None, Some(error.to_string())),
    };

    let mut joins = Vec::new();
    // Rows of the second response, classified the same way; their own
    // provisional reasons were already recorded from the first response.
    let mut ignored = Vec::new();
    let second_candidates = second
        .as_ref()
        .map(|snapshot| candidates(snapshot, &mut ignored))
        .unwrap_or_default();

    for (pid, (session_id, row)) in &first_candidates {
        let reason = 'check: {
            let Some(second) = second.as_ref() else {
                break 'check Some("SECOND_LOOKUP_FAILED");
            };
            let before_result = &before[pid];
            let after_result = &after[pid];
            if let Some(code) = sample_code(before_result).or(sample_code(after_result)) {
                break 'check Some(code);
            }
            let (Ok(before), Ok(after)) = (before_result, after_result) else {
                break 'check Some("PROCESS_UNAVAILABLE");
            };
            if !before.sample.same_incarnation(&after.sample) {
                break 'check Some("PROCESS_CHANGED");
            }
            if before.executable != after.executable {
                break 'check Some("EXECUTABLE_CHANGED");
            }
            if !qualifies(&after.executable.path) {
                break 'check Some("EXECUTABLE_UNQUALIFIED");
            }
            let Some(device) = before.sample.controlling_device else {
                break 'check Some("NO_CONTROLLING_TERMINAL");
            };
            if after.sample.controlling_device != Some(device) {
                break 'check Some("DEVICE_CHANGED");
            }
            let pid_rows = second.rows_for_pid(*pid);
            if pid_rows.is_empty() {
                break 'check Some("ROW_ABSENT_IN_SECOND_LOOKUP");
            }
            match second_candidates.get(pid) {
                None => break 'check Some("ROW_UNQUALIFIED_IN_SECOND_LOOKUP"),
                Some((second_id, _)) if second_id != session_id => {
                    break 'check Some("SESSION_CHANGED_BETWEEN_LOOKUPS");
                }
                Some(_) => {}
            }
            joins.push(Join {
                native_session_id: session_id.clone(),
                pid: *pid,
                before: before.clone(),
                after: after.clone(),
                device,
            });
            None
        };
        if let Some(reason) = reason {
            provisional_rows.push(provisional(row, reason));
        }
    }
    // PIDs that appear only in the confirming response wait for a later pass.
    for (pid, (_, row)) in &second_candidates {
        if !first_candidates.contains_key(pid) {
            provisional_rows.push(provisional(row, "NEW_IN_SECOND_LOOKUP"));
        }
    }

    let latest = second.as_ref().unwrap_or(&first);
    let mut seen = BTreeSet::new();
    let sessions = latest
        .rows
        .iter()
        .filter_map(|row| {
            let id = row.full_session_id()?;
            seen.insert(id.to_owned()).then(|| ObservedSession {
                native_session_id: id.to_owned(),
                kind: row.kind.clone(),
                status: row.status.clone(),
                waiting_for: row.waiting_for.clone(),
                name: row.name.clone(),
            })
        })
        .collect();

    Ok(DiscoveryPass {
        first,
        second,
        second_error,
        joins,
        provisional: provisional_rows,
        sessions,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;
    use std::collections::{HashMap, VecDeque};

    use threadspace_surfaces_macos::process::{ExecutableIdentity, ProcessSample};

    use super::*;

    pub const VERSIONS: &str = "/h/.local/share/claude/versions";

    pub fn claude(pid: i32, birth: u64, device: Option<u32>) -> Incarnation {
        Incarnation {
            sample: ProcessSample {
                pid,
                ppid: 100,
                uid: 501,
                start_seconds: birth,
                start_microseconds: 7,
                controlling_device: device,
                pgid: pid as u32,
                tpgid: pid as u32,
                status: 3,
                comm: "claude".into(),
            },
            executable: ExecutableIdentity {
                path: format!("{VERSIONS}/2.1.291"),
                file_id: Some("16777234:42".into()),
            },
        }
    }

    pub fn row(pid: i64, session: &str) -> InventoryRow {
        InventoryRow {
            kind: Some("interactive".into()),
            pid: Some(pid),
            session_id: Some(session.into()),
            status: Some("idle".into()),
            ..InventoryRow::default()
        }
    }

    pub fn snapshot(rows: Vec<InventoryRow>) -> InventorySnapshot {
        InventorySnapshot {
            rows,
            request_started_ms: 1,
            request_ended_ms: 2,
            binary: format!("{VERSIONS}/2.1.291"),
        }
    }

    /// Returns queued snapshots in order.
    pub struct Script(pub RefCell<VecDeque<Result<InventorySnapshot, InventoryError>>>);

    impl Inventory for Script {
        fn fetch(&self) -> Result<InventorySnapshot, InventoryError> {
            self.0
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err(InventoryError::Parse("script exhausted".into())))
        }
    }

    /// Returns queued samples per PID in order; the last one repeats.
    pub struct Kernel(pub RefCell<HashMap<i32, VecDeque<Result<Incarnation, ProcessError>>>>);

    impl Kernel {
        pub fn new(entries: Vec<(i32, Vec<Result<Incarnation, ProcessError>>)>) -> Self {
            Self(RefCell::new(
                entries
                    .into_iter()
                    .map(|(pid, samples)| (pid, samples.into()))
                    .collect(),
            ))
        }
    }

    impl Sampler for Kernel {
        fn incarnation(&self, pid: i32) -> Result<Incarnation, ProcessError> {
            let mut map = self.0.borrow_mut();
            let Some(queue) = map.get_mut(&pid) else {
                return Err(ProcessError::Vanished { pid });
            };
            if queue.len() > 1 {
                queue
                    .pop_front()
                    .unwrap_or(Err(ProcessError::Vanished { pid }))
            } else {
                queue
                    .front()
                    .cloned()
                    .unwrap_or(Err(ProcessError::Vanished { pid }))
            }
        }
    }

    fn qualifies(path: &str) -> bool {
        path.starts_with(VERSIONS)
    }

    fn run(
        first: Vec<InventoryRow>,
        second: Result<Vec<InventoryRow>, InventoryError>,
        kernel: Kernel,
    ) -> DiscoveryPass {
        let script = Script(RefCell::new(
            vec![Ok(snapshot(first)), second.map(snapshot)].into(),
        ));
        discover(&script, &kernel, qualifies).expect("first lookup")
    }

    fn reasons(pass: &DiscoveryPass) -> Vec<&'static str> {
        pass.provisional.iter().map(|p| p.reason).collect()
    }

    #[test]
    fn three_same_cwd_sessions_join_independently() {
        let first = vec![row(11, "A"), row(12, "B"), row(13, "C")];
        let kernel = Kernel::new(vec![
            (11, vec![Ok(claude(11, 1000, Some(0x1000001)))]),
            (12, vec![Ok(claude(12, 1001, Some(0x1000002)))]),
            (13, vec![Ok(claude(13, 1002, Some(0x1000003)))]),
        ]);
        let pass = run(first.clone(), Ok(first), kernel);
        assert_eq!(pass.joins.len(), 3);
        assert!(pass.provisional.is_empty(), "{:?}", pass.provisional);
        let ids: Vec<&str> = pass
            .joins
            .iter()
            .map(|j| j.native_session_id.as_str())
            .collect();
        assert_eq!(ids, vec!["A", "B", "C"]);
        assert_eq!(pass.sessions.len(), 3);
    }

    #[test]
    fn pid_reused_before_the_first_sample_cannot_inherit_the_old_row() {
        // The first response still names a dead process's session for PID 11;
        // by the incumbent sample PID 11 is a new claude whose own session the
        // second response reports.
        let kernel = Kernel::new(vec![(11, vec![Ok(claude(11, 2000, Some(5)))])]);
        let pass = run(vec![row(11, "OLD")], Ok(vec![row(11, "NEW")]), kernel);
        assert!(pass.joins.is_empty());
        assert_eq!(reasons(&pass), vec!["SESSION_CHANGED_BETWEEN_LOOKUPS"]);
    }

    #[test]
    fn same_pid_with_a_different_birth_across_the_bracket_is_rejected() {
        let kernel = Kernel::new(vec![(
            11,
            vec![Ok(claude(11, 1000, Some(5))), Ok(claude(11, 1999, Some(5)))],
        )]);
        let pass = run(vec![row(11, "A")], Ok(vec![row(11, "A")]), kernel);
        assert!(pass.joins.is_empty());
        assert_eq!(reasons(&pass), vec!["PROCESS_CHANGED"]);
    }

    #[test]
    fn executable_replacement_within_the_same_pid_and_birth_is_rejected() {
        let mut replaced = claude(11, 1000, Some(5));
        replaced.executable.path = "/bin/zsh".into();
        let kernel = Kernel::new(vec![(
            11,
            vec![Ok(claude(11, 1000, Some(5))), Ok(replaced)],
        )]);
        let pass = run(vec![row(11, "A")], Ok(vec![row(11, "A")]), kernel);
        assert_eq!(reasons(&pass), vec!["EXECUTABLE_CHANGED"]);
    }

    #[test]
    fn unqualified_executables_and_missing_terminals_stay_provisional() {
        let mut node = claude(11, 1000, Some(5));
        node.executable.path = "/opt/homebrew/bin/node".into();
        let kernel = Kernel::new(vec![
            (11, vec![Ok(node)]),
            (12, vec![Ok(claude(12, 1000, None))]),
        ]);
        let rows = vec![row(11, "A"), row(12, "B")];
        let pass = run(rows.clone(), Ok(rows), kernel);
        assert!(pass.joins.is_empty());
        assert_eq!(
            reasons(&pass),
            vec!["EXECUTABLE_UNQUALIFIED", "NO_CONTROLLING_TERMINAL"]
        );
    }

    #[test]
    fn device_change_vanishing_and_denial_are_explicit() {
        let kernel = Kernel::new(vec![
            (
                11,
                vec![Ok(claude(11, 1000, Some(5))), Ok(claude(11, 1000, Some(6)))],
            ),
            (
                12,
                vec![
                    Ok(claude(12, 1000, Some(7))),
                    Err(ProcessError::Vanished { pid: 12 }),
                ],
            ),
            (13, vec![Err(ProcessError::Denied { pid: 13, errno: 1 })]),
        ]);
        let rows = vec![row(11, "A"), row(12, "B"), row(13, "C")];
        let pass = run(rows.clone(), Ok(rows), kernel);
        assert!(pass.joins.is_empty());
        assert_eq!(
            reasons(&pass),
            vec!["DEVICE_CHANGED", "PROCESS_VANISHED", "PROCESS_READ_DENIED"]
        );
    }

    #[test]
    fn background_missing_duplicate_and_new_rows_never_join() {
        let background = InventoryRow {
            kind: Some("background".into()),
            id: Some("ab12".into()),
            session_id: Some("BG".into()),
            ..InventoryRow::default()
        };
        let no_id = InventoryRow {
            kind: Some("interactive".into()),
            pid: Some(14),
            ..InventoryRow::default()
        };
        let kernel = Kernel::new(vec![
            (11, vec![Ok(claude(11, 1000, Some(5)))]),
            (15, vec![Ok(claude(15, 1000, Some(9)))]),
        ]);
        let first = vec![background.clone(), no_id, row(11, "A"), row(11, "B")];
        let second = vec![background, row(11, "A"), row(11, "B"), row(15, "E")];
        let pass = run(first, Ok(second), kernel);
        assert!(pass.joins.is_empty());
        assert_eq!(
            reasons(&pass),
            vec![
                "UNSUPPORTED_KIND",
                "MISSING_FULL_SESSION_ID",
                "DUPLICATE_PID",
                "DUPLICATE_PID",
                "NEW_IN_SECOND_LOOKUP"
            ]
        );
        // The background row's full session ID is still an identity fact.
        assert!(pass.sessions.iter().any(|s| s.native_session_id == "BG"));
    }

    #[test]
    fn a_failed_or_absent_confirmation_never_joins() {
        let kernel = Kernel::new(vec![(11, vec![Ok(claude(11, 1000, Some(5)))])]);
        let failed = run(
            vec![row(11, "A")],
            Err(InventoryError::Failed {
                status: Some(1),
                timed_out: false,
                stderr: String::new(),
            }),
            Kernel::new(vec![(11, vec![Ok(claude(11, 1000, Some(5)))])]),
        );
        assert_eq!(reasons(&failed), vec!["SECOND_LOOKUP_FAILED"]);
        let absent = run(vec![row(11, "A")], Ok(vec![]), kernel);
        assert_eq!(reasons(&absent), vec!["ROW_ABSENT_IN_SECOND_LOOKUP"]);
    }

    #[test]
    fn one_session_in_two_processes_is_two_joins() {
        let rows = vec![row(11, "A"), row(12, "A")];
        let kernel = Kernel::new(vec![
            (11, vec![Ok(claude(11, 1000, Some(5)))]),
            (12, vec![Ok(claude(12, 1001, Some(6)))]),
        ]);
        let pass = run(rows.clone(), Ok(rows), kernel);
        assert_eq!(
            pass.joins.len(),
            2,
            "multiple attachments are retained, not chosen"
        );
        assert_eq!(pass.sessions.len(), 1);
    }
}
