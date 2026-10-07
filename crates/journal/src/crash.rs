//! Qualification-only crash points on the admission path (M0C, G10). This
//! module exists only with the `qualification` feature, so production
//! builds carry neither the points nor the environment lookup.
//!
//! A crash is immediate process death: the process sends itself `SIGKILL`.
//! Like `abort()`, that runs no unwinding, destructors, `atexit` handlers or
//! stdio flushes, and the kernel closes descriptors and releases `flock` and
//! SQLite's POSIX locks exactly as for any other abnormal exit. `SIGKILL` is
//! used instead of `SIGABRT` so that repeated qualification runs do not
//! produce a macOS crash report per death.

use std::str::FromStr;

use crate::JournalError;

/// Selects the crash point, e.g. `in-transaction`.
pub const CRASH_POINT_ENV: &str = "THREADSPACE_QUALIFY_CRASH_POINT";
/// 1-based admission call at which the selected point fires (default 1).
pub const CRASH_AT_ENV: &str = "THREADSPACE_QUALIFY_CRASH_AT";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashPoint {
    /// Before the admission transaction begins; nothing is written.
    BeforeTransaction,
    /// The record's rows are written but COMMIT has not run.
    InTransaction,
    /// Canonical admission: facts and identity assignments journaled, not yet reduced.
    AfterFacts,
    /// Canonical admission: reduced and materialized, COMMIT not yet run.
    AfterReduce,
    /// COMMIT returned; the receipt has not been returned to the caller.
    AfterCommitBeforeReceipt,
}

impl CrashPoint {
    pub const ALL: [Self; 3] = [
        Self::BeforeTransaction,
        Self::InTransaction,
        Self::AfterCommitBeforeReceipt,
    ];

    /// Every position of the canonical admission transaction (M1).
    pub const CANONICAL: [Self; 5] = [
        Self::BeforeTransaction,
        Self::InTransaction,
        Self::AfterFacts,
        Self::AfterReduce,
        Self::AfterCommitBeforeReceipt,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::BeforeTransaction => "before-transaction",
            Self::InTransaction => "in-transaction",
            Self::AfterFacts => "after-facts",
            Self::AfterReduce => "after-reduce",
            Self::AfterCommitBeforeReceipt => "after-commit-before-receipt",
        }
    }
}

impl FromStr for CrashPoint {
    type Err = JournalError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::CANONICAL
            .into_iter()
            .find(|point| point.name() == value)
            .ok_or_else(|| JournalError::Invalid {
                detail: format!("unknown crash point {value:?}"),
            })
    }
}

/// Kills the process at `point` on the `at_admission`-th admission call
/// counted from when the plan was armed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrashPlan {
    pub point: CrashPoint,
    pub at_admission: u64,
}

impl CrashPlan {
    /// Reads `THREADSPACE_QUALIFY_CRASH_POINT` and `THREADSPACE_QUALIFY_CRASH_AT`.
    pub fn from_env() -> Result<Option<Self>, JournalError> {
        let Ok(point) = std::env::var(CRASH_POINT_ENV) else {
            return Ok(None);
        };
        let point = point.parse()?;
        let at_admission = match std::env::var(CRASH_AT_ENV) {
            Ok(text) => text
                .parse::<u64>()
                .ok()
                .filter(|at| *at > 0)
                .ok_or_else(|| JournalError::Invalid {
                    detail: format!("{CRASH_AT_ENV} must be a positive integer, not {text:?}"),
                })?,
            Err(_) => 1,
        };
        Ok(Some(Self {
            point,
            at_admission,
        }))
    }
}

#[derive(Debug, Default)]
pub(crate) struct CrashState {
    plan: Option<CrashPlan>,
    admissions: u64,
}

impl CrashState {
    pub(crate) fn from_env() -> Result<Self, JournalError> {
        Ok(Self {
            plan: CrashPlan::from_env()?,
            admissions: 0,
        })
    }

    pub(crate) fn arm(&mut self, plan: Option<CrashPlan>) {
        self.plan = plan;
        self.admissions = 0;
    }

    /// Counts one admission call and returns the point armed for it, if any.
    pub(crate) fn next_admission(&mut self) -> Option<CrashPoint> {
        self.admissions += 1;
        self.plan
            .filter(|plan| plan.at_admission == self.admissions)
            .map(|plan| plan.point)
    }
}

impl crate::Journal {
    /// Arms (or with `None`, disarms) a crash point, replacing any plan read
    /// from the environment at open. Admissions are counted from this call.
    pub fn set_crash_plan(&mut self, plan: Option<CrashPlan>) {
        self.crash.arm(plan);
    }
}

/// Dies here when `armed` is `point`.
pub(crate) fn hit(armed: Option<CrashPoint>, point: CrashPoint) {
    if armed == Some(point) {
        die();
    }
}

fn die() -> ! {
    // SAFETY: kill(2) with our own PID and a constant signal, and pause(2),
    // have no memory effects. SIGKILL cannot be caught or blocked.
    unsafe {
        libc::kill(libc::getpid(), libc::SIGKILL);
    }
    // A process-directed signal can be taken by another thread, so kill(2)
    // may return before the process is torn down; this thread does nothing
    // further either way.
    loop {
        // SAFETY: see above.
        unsafe {
            libc::pause();
        }
    }
}
