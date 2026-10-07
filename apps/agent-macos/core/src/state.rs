//! In-memory gates shared by the companion's threads. The single writer owns
//! the persisted values (observation preference, maintenance phase); this is
//! their mirror, read by discovery and the control server to decide whether
//! capture admission is open (SPEC §18.9, §19.5).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use threadspace_contracts::control::MaintenancePhase;
use threadspace_contracts::diagnostics::{LaunchProvenance, PowerHistory};

pub struct Runtime {
    /// The owner's persisted preference; never an authorization by itself.
    observation_enabled: AtomicBool,
    /// Unknown until classified at startup, so nothing is supervised by
    /// default (SPEC §18.9).
    provenance: Mutex<LaunchProvenance>,
    maintenance: Mutex<MaintenancePhase>,
    asleep: AtomicBool,
    power: Mutex<PowerHistory>,
    #[cfg(feature = "qualification")]
    fail_next_backup: AtomicBool,
}

pub static RUNTIME: Runtime = Runtime::new();

impl Runtime {
    const fn new() -> Self {
        Self {
            observation_enabled: AtomicBool::new(true),
            provenance: Mutex::new(LaunchProvenance::Unknown),
            maintenance: Mutex::new(MaintenancePhase::None),
            asleep: AtomicBool::new(false),
            power: Mutex::new(PowerHistory {
                sleeps: 0,
                wakes: 0,
                last_sleep_wall_ms: None,
                last_wake_wall_ms: None,
                boot_id_at_last_wake: None,
                wake_revalidations: 0,
            }),
            #[cfg(feature = "qualification")]
            fail_next_backup: AtomicBool::new(false),
        }
    }

    pub fn observation_enabled(&self) -> bool {
        self.observation_enabled.load(Ordering::Acquire)
    }

    pub fn set_observation_enabled(&self, enabled: bool) {
        self.observation_enabled.store(enabled, Ordering::Release);
    }

    pub fn provenance(&self) -> LaunchProvenance {
        self.provenance
            .lock()
            .map(|provenance| *provenance)
            .unwrap_or(LaunchProvenance::Unknown)
    }

    pub fn set_provenance(&self, provenance: LaunchProvenance) {
        if let Ok(mut current) = self.provenance.lock() {
            *current = provenance;
        }
    }

    /// Only the login item's own companion is supervised: a notification
    /// cold start or an unknown launch never becomes the observer.
    pub fn supervised(&self) -> bool {
        self.provenance() == LaunchProvenance::LoginItem
    }

    pub fn maintenance(&self) -> MaintenancePhase {
        self.maintenance
            .lock()
            .map(|phase| *phase)
            .unwrap_or(MaintenancePhase::Prepared)
    }

    pub fn set_maintenance(&self, phase: MaintenancePhase) {
        if let Ok(mut current) = self.maintenance.lock() {
            *current = phase;
        }
    }

    pub fn asleep(&self) -> bool {
        self.asleep.load(Ordering::Acquire)
    }

    /// Provider polling and capture admission run only in the supervised
    /// companion, while observation is enabled, no maintenance phase is active
    /// and the machine is awake.
    pub fn admission_open(&self) -> bool {
        self.observation_enabled()
            && self.supervised()
            && self.maintenance() == MaintenancePhase::None
            && !self.asleep()
    }

    /// Owner mutations are refused only while maintenance holds the store.
    pub fn writes_open(&self) -> bool {
        self.maintenance() == MaintenancePhase::None
    }

    pub fn note_sleep(&self, wall_ms: i64) {
        self.asleep.store(true, Ordering::Release);
        if let Ok(mut power) = self.power.lock() {
            power.sleeps += 1;
            power.last_sleep_wall_ms = Some(wall_ms);
        }
    }

    pub fn note_wake(&self, wall_ms: i64, boot_id: Option<String>) {
        self.asleep.store(false, Ordering::Release);
        if let Ok(mut power) = self.power.lock() {
            power.wakes += 1;
            power.last_wake_wall_ms = Some(wall_ms);
            power.boot_id_at_last_wake = boot_id;
        }
    }

    pub fn note_wake_revalidation(&self) {
        if let Ok(mut power) = self.power.lock() {
            power.wake_revalidations += 1;
        }
    }

    pub fn power(&self) -> PowerHistory {
        self.power
            .lock()
            .map(|power| power.clone())
            .unwrap_or_default()
    }

    #[cfg(feature = "qualification")]
    pub fn arm_backup_failure(&self) {
        self.fail_next_backup.store(true, Ordering::Release);
    }

    /// Consumes an armed one-shot backup failure.
    pub fn take_backup_failure(&self) -> bool {
        #[cfg(feature = "qualification")]
        {
            self.fail_next_backup.swap(false, Ordering::AcqRel)
        }
        #[cfg(not(feature = "qualification"))]
        {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_supervised_before_classification() {
        let runtime = Runtime::new();
        assert!(runtime.observation_enabled());
        assert_eq!(runtime.provenance(), LaunchProvenance::Unknown);
        assert!(!runtime.supervised());
        assert!(!runtime.admission_open());
    }

    #[test]
    fn the_enabled_preference_alone_never_opens_admission() {
        for provenance in [LaunchProvenance::LaunchServices, LaunchProvenance::Unknown] {
            let runtime = Runtime::new();
            runtime.set_observation_enabled(true);
            runtime.set_provenance(provenance);
            assert!(!runtime.admission_open(), "{provenance:?} must not admit");
            // Owner mutations are a separate gate (control-only behavior).
            assert!(runtime.writes_open());
        }
    }

    #[test]
    fn admission_needs_preference_supervision_no_maintenance_and_awake() {
        let runtime = Runtime::new();
        runtime.set_provenance(LaunchProvenance::LoginItem);
        assert!(runtime.admission_open());
        runtime.set_observation_enabled(false);
        assert!(!runtime.admission_open());
        runtime.set_observation_enabled(true);
        runtime.set_maintenance(MaintenancePhase::Prepared);
        assert!(!runtime.admission_open());
        runtime.set_maintenance(MaintenancePhase::None);
        runtime.note_sleep(1);
        assert!(!runtime.admission_open());
        runtime.note_wake(2, None);
        assert!(runtime.admission_open());
    }
}
