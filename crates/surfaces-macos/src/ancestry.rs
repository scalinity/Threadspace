//! Bounded, per-edge-validated process ancestry (SPEC §4.5 step 2). Each edge
//! samples the child, then the parent, then re-reads both: the child's
//! incarnation and PPID and the parent's incarnation must be unchanged, and
//! the parent must have been born no later than the child. A vanished,
//! reparented, reused or cyclic link stops the walk; a stable PID further up
//! cannot repair an unproven edge. Hooks may be detached from the provider's
//! terminal, so nothing here reads stdin or `/dev/tty`.

use crate::process::{Incarnation, ProcessError, sample_incarnation};

pub const MAX_ANCESTORS: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AncestryStop {
    /// Reached launchd (PID 1) or the kernel (PID 0) with every edge proven.
    ReachedRoot,
    /// A link could not be sampled.
    Unreadable { pid: i32, code: &'static str },
    /// The child's PPID changed between reads (reparented or exiting).
    Reparented { pid: i32 },
    /// The parent's incarnation changed, or it was born after the child.
    ParentReused { pid: i32 },
    /// A PID repeated in the chain.
    Cycle { pid: i32 },
    /// The bound was reached before the root.
    DepthLimit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ancestry {
    /// `chain[0]` is the starting process; each next entry is its proven parent.
    pub chain: Vec<Incarnation>,
    pub stop: AncestryStop,
}

/// Process sampling, abstracted so synthetic chains can exercise every stop.
pub trait Sampler {
    fn incarnation(&self, pid: i32) -> Result<Incarnation, ProcessError>;
}

pub struct KernelSampler;

impl Sampler for KernelSampler {
    fn incarnation(&self, pid: i32) -> Result<Incarnation, ProcessError> {
        sample_incarnation(pid)
    }
}

fn unreadable(pid: i32, error: &ProcessError) -> AncestryStop {
    AncestryStop::Unreadable {
        pid,
        code: error.code(),
    }
}

/// Walks from `start` toward launchd, validating every edge, for at most
/// `max_links` parents.
pub fn walk(sampler: &impl Sampler, start: i32, max_links: usize) -> Ancestry {
    let mut chain = Vec::new();
    let mut child = match sampler.incarnation(start) {
        Ok(first) => first,
        Err(error) => {
            return Ancestry {
                chain,
                stop: unreadable(start, &error),
            };
        }
    };
    chain.push(child.clone());
    for _ in 0..max_links {
        let parent_pid = child.sample.ppid;
        if parent_pid <= 1 {
            return Ancestry {
                chain,
                stop: AncestryStop::ReachedRoot,
            };
        }
        if chain.iter().any(|link| link.sample.pid == parent_pid) {
            return Ancestry {
                chain,
                stop: AncestryStop::Cycle { pid: parent_pid },
            };
        }
        let parent = match sampler.incarnation(parent_pid) {
            Ok(parent) => parent,
            Err(error) => {
                return Ancestry {
                    chain,
                    stop: unreadable(parent_pid, &error),
                };
            }
        };
        let child_again = match sampler.incarnation(child.sample.pid) {
            Ok(again) => again,
            Err(error) => {
                return Ancestry {
                    chain,
                    stop: unreadable(child.sample.pid, &error),
                };
            }
        };
        if !child_again.same_process_and_image(&child) || child_again.sample.ppid != parent_pid {
            return Ancestry {
                chain,
                stop: AncestryStop::Reparented {
                    pid: child.sample.pid,
                },
            };
        }
        let parent_again = match sampler.incarnation(parent_pid) {
            Ok(again) => again,
            Err(error) => {
                return Ancestry {
                    chain,
                    stop: unreadable(parent_pid, &error),
                };
            }
        };
        if !parent_again.same_process_and_image(&parent)
            || parent.sample.birth_micros() > child.sample.birth_micros()
        {
            return Ancestry {
                chain,
                stop: AncestryStop::ParentReused { pid: parent_pid },
            };
        }
        chain.push(parent.clone());
        child = parent;
    }
    Ancestry {
        chain,
        stop: AncestryStop::DepthLimit,
    }
}

/// The nearest proven ancestor accepted by the adapter's qualification rule,
/// rechecked at selection time. Unrelated or unproven ancestors are never
/// substituted.
pub fn select_provider<'a>(
    sampler: &impl Sampler,
    ancestry: &'a Ancestry,
    qualifies: impl Fn(&Incarnation) -> bool,
) -> Option<&'a Incarnation> {
    let selected = ancestry.chain.iter().skip(1).find(|link| qualifies(link))?;
    let again = sampler.incarnation(selected.sample.pid).ok()?;
    again.same_process_and_image(selected).then_some(selected)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;
    use crate::process::{ExecutableIdentity, ProcessSample};

    fn link(pid: i32, ppid: i32, birth: u64, exe: &str) -> Incarnation {
        Incarnation {
            sample: ProcessSample {
                pid,
                ppid,
                uid: 501,
                start_seconds: birth,
                start_microseconds: 0,
                controlling_device: None,
                pgid: pid as u32,
                tpgid: 0,
                status: 3,
                comm: exe.into(),
            },
            executable: ExecutableIdentity {
                path: format!("/bin/{exe}"),
                file_id: Some("1:1".into()),
            },
        }
    }

    /// A synthetic process table; `after` replaces an entry after it has been
    /// read `reads` times, simulating a change between the edge's reads.
    struct Table {
        rows: HashMap<i32, Incarnation>,
        after: HashMap<i32, (usize, Option<Incarnation>)>,
        reads: RefCell<HashMap<i32, usize>>,
    }

    impl Table {
        fn new(rows: &[Incarnation]) -> Self {
            Self {
                rows: rows
                    .iter()
                    .map(|row| (row.sample.pid, row.clone()))
                    .collect(),
                after: HashMap::new(),
                reads: RefCell::new(HashMap::new()),
            }
        }
    }

    impl Sampler for Table {
        fn incarnation(&self, pid: i32) -> Result<Incarnation, ProcessError> {
            let mut reads = self.reads.borrow_mut();
            let count = reads.entry(pid).or_insert(0);
            *count += 1;
            if let Some((threshold, replacement)) = self.after.get(&pid)
                && *count > *threshold
            {
                return replacement.clone().ok_or(ProcessError::Vanished { pid });
            }
            self.rows
                .get(&pid)
                .cloned()
                .ok_or(ProcessError::Vanished { pid })
        }
    }

    #[test]
    fn proves_every_edge_to_the_root() {
        let table = Table::new(&[
            link(400, 300, 40, "probe"),
            link(300, 200, 30, "sh"),
            link(200, 100, 20, "claude"),
            link(100, 1, 10, "zsh"),
        ]);
        let ancestry = walk(&table, 400, MAX_ANCESTORS);
        assert_eq!(ancestry.stop, AncestryStop::ReachedRoot);
        let pids: Vec<i32> = ancestry.chain.iter().map(|l| l.sample.pid).collect();
        assert_eq!(pids, vec![400, 300, 200, 100]);
        let provider = select_provider(&table, &ancestry, |l| l.sample.comm == "claude");
        assert_eq!(provider.map(|l| l.sample.pid), Some(200));
    }

    #[test]
    fn a_parent_born_after_its_child_is_a_reused_pid() {
        let table = Table::new(&[link(400, 300, 40, "probe"), link(300, 1, 50, "claude")]);
        let ancestry = walk(&table, 400, MAX_ANCESTORS);
        assert_eq!(ancestry.stop, AncestryStop::ParentReused { pid: 300 });
        assert_eq!(
            ancestry.chain.len(),
            1,
            "the unproven parent is not in the chain"
        );
        assert!(select_provider(&table, &ancestry, |l| l.sample.comm == "claude").is_none());
    }

    #[test]
    fn reparenting_between_reads_stops_the_walk() {
        let mut table = Table::new(&[link(400, 300, 40, "probe"), link(300, 1, 30, "claude")]);
        // The child's second read shows it reparented to launchd.
        table
            .after
            .insert(400, (1, Some(link(400, 1, 40, "probe"))));
        let ancestry = walk(&table, 400, MAX_ANCESTORS);
        assert_eq!(ancestry.stop, AncestryStop::Reparented { pid: 400 });
    }

    #[test]
    fn a_parent_replaced_mid_edge_is_rejected() {
        let mut table = Table::new(&[link(400, 300, 40, "probe"), link(300, 1, 30, "claude")]);
        table
            .after
            .insert(300, (1, Some(link(300, 1, 35, "claude"))));
        let ancestry = walk(&table, 400, MAX_ANCESTORS);
        assert_eq!(ancestry.stop, AncestryStop::ParentReused { pid: 300 });
    }

    #[test]
    fn vanished_cyclic_and_deep_chains_stop_honestly() {
        let table = Table::new(&[link(400, 300, 40, "probe")]);
        assert!(matches!(
            walk(&table, 400, MAX_ANCESTORS).stop,
            AncestryStop::Unreadable { pid: 300, .. }
        ));
        let cyclic = Table::new(&[link(400, 300, 40, "a"), link(300, 400, 30, "b")]);
        assert_eq!(
            walk(&cyclic, 400, MAX_ANCESTORS).stop,
            AncestryStop::Cycle { pid: 400 }
        );
        let deep = Table::new(&[link(4, 3, 4, "a"), link(3, 2, 3, "b"), link(2, 1, 2, "c")]);
        assert_eq!(walk(&deep, 4, 1).stop, AncestryStop::DepthLimit);
    }

    /// Natively, a Terminal lineage passes through root-owned `login`, whose
    /// BSD info an unprivileged reader is denied: the walk must stop there
    /// honestly rather than skip the link.
    #[test]
    fn walks_this_test_process_natively() {
        let ancestry = walk(&KernelSampler, std::process::id() as i32, MAX_ANCESTORS);
        assert!(ancestry.chain.len() >= 2, "{ancestry:?}");
        assert!(
            matches!(
                ancestry.stop,
                AncestryStop::ReachedRoot
                    | AncestryStop::DepthLimit
                    | AncestryStop::Unreadable {
                        code: "PROCESS_READ_DENIED",
                        ..
                    }
            ),
            "{:?}",
            ancestry.stop
        );
        for edge in ancestry.chain.windows(2) {
            assert_eq!(edge[0].sample.ppid, edge[1].sample.pid);
            assert!(edge[1].sample.birth_micros() <= edge[0].sample.birth_micros());
        }
    }
}
