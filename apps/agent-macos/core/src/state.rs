//! In-memory gates shared by the companion's threads. The single writer owns
//! the persisted values (observation preference, maintenance phase); this is
//! their mirror, read by discovery and the control server to decide whether
//! capture admission is open (SPEC §19.5).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use threadspace_contracts::control::MaintenancePhase;
use threadspace_contracts::diagnostics::PowerHistory;

pub struct Runtime {
    observation_enabled: AtomicBool,
    /// False only for a companion its login item did not start (a
    /// notification cold start), which never becomes the enabled writer.
    supervised: AtomicBool,
    maintenance: Mutex<MaintenancePhase>,
    asleep: AtomicBool,
    power: Mutex<PowerHistory>,
    #[cfg(feature = "qualification")]
    fail_next_backup: AtomicBool,
}

pub static RUNTIME: Runtime = Runtime {
    observation_enabled: AtomicBool::new(true),
    supervised: AtomicBool::new(true),
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
};

impl Runtime {
    pub fn observation_enabled(&self) -> bool {
        self.observation_enabled.load(Ordering::Acquire)
    }

    pub fn set_observation_enabled(&self, enabled: bool) {
        self.observation_enabled.store(enabled, Ordering::Release);
    }

    pub fn supervised(&self) -> bool {
        self.supervised.load(Ordering::Acquire)
    }

    pub fn set_supervised(&self, supervised: bool) {
        self.supervised.store(supervised, Ordering::Release);
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

    /// Provider polling and capture admission run only while observation is
    /// enabled, no maintenance phase is active and the machine is awake.
    pub fn admission_open(&self) -> bool {
        self.observation_enabled() && self.maintenance() == MaintenancePhase::None && !self.asleep()
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
