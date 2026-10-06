//! Native office-view incarnation markers (SPEC §18.3). At construction the
//! office WebView receives an application-owned marker in its public resource
//! table. Every command re-reads that marker from the incoming WebView; label
//! equality cannot certify the same view, because a recreated `office` view
//! has a fresh resource table.

use std::sync::Mutex;

use tauri::{Manager, ResourceId, Runtime, Webview};
use threadspace_contracts::ui::{UiError, UiErrorCode};
use uuid::Uuid;

pub struct OfficeIncarnation {
    pub id: Uuid,
}

impl tauri::Resource for OfficeIncarnation {}

#[derive(Debug, Clone, Copy)]
struct ActiveView {
    id: Uuid,
    rid: ResourceId,
}

#[derive(Default)]
pub struct ViewRegistry {
    active: Mutex<Option<ActiveView>>,
}

impl ViewRegistry {
    pub fn activate(&self, id: Uuid, rid: ResourceId) {
        if let Ok(mut active) = self.active.lock() {
            *active = Some(ActiveView { id, rid });
        }
    }

    pub fn retire(&self, id: Uuid) {
        if let Ok(mut active) = self.active.lock()
            && active.is_some_and(|view| view.id == id)
        {
            *active = None;
        }
    }

    /// Returns the caller's incarnation only if it is the active office view.
    pub fn verify<R: Runtime>(&self, webview: &Webview<R>) -> Result<Uuid, UiError> {
        let stale = || {
            UiError::new(
                UiErrorCode::StaleView,
                "calling view is not the active office incarnation",
            )
        };
        let active = self
            .active
            .lock()
            .ok()
            .and_then(|active| *active)
            .ok_or_else(stale)?;
        let marker = webview
            .resources_table()
            .get::<OfficeIncarnation>(active.rid)
            .map_err(|_| stale())?;
        if marker.id != active.id {
            return Err(stale());
        }
        Ok(active.id)
    }
}
