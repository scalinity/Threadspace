//! Native operations answered under the companion's own identity: diagnostics,
//! integration status, and the explicit notification/automation setup steps
//! (SPEC §13.5, §19.1). Terminal is never launched to answer a probe.

use std::path::PathBuf;
use std::sync::mpsc::{self, SyncSender};
use std::time::Duration;

use serde_json::json;
use threadspace_contracts::control::{
    ControlError, ControlErrorCode, ControlRequestBody, ControlResponseBody,
};
use threadspace_contracts::diagnostics::{
    AutomationPermission, CompanionDiagnostics, CompanionIntegration, ProcessIdentity,
    TerminalIntegration,
};
use threadspace_relay::paths::redact_home;
use threadspace_surfaces_macos::terminal::{self, TERMINAL_APP_PATH, TERMINAL_BUNDLE_ID};

use crate::bridge::{self, BridgeEvent, BridgeRequest};
use crate::discovery::{DiscoveryContext, Trigger};
use crate::log;
use crate::route;
use crate::server::CoreContext;
use crate::writer::WriterCommand;

const QUICK: Duration = Duration::from_secs(5);
/// Upper bound on waiting for the owner to answer a native permission prompt.
const PROMPT: Duration = Duration::from_secs(180);

// Apple-event status codes from `AEDeterminePermissionToAutomateTarget`.
const NO_ERR: i32 = 0;
const ERR_AE_EVENT_NOT_PERMITTED: i32 = -1743;
const ERR_AE_EVENT_WOULD_REQUIRE_USER_CONSENT: i32 = -1744;
const PROC_NOT_FOUND: i32 = -600;

/// The parts of the core context a worker thread needs.
pub struct OpsContext {
    bundle_identifier: String,
    core_generation: String,
    store_generation: String,
    identity: ProcessIdentity,
    started_at_ms: i64,
    resources_dir: PathBuf,
    claude: DiscoveryContext,
    discovery: SyncSender<Trigger>,
    /// When the request reached the companion.
    received_ms: i64,
}

impl From<&CoreContext> for OpsContext {
    fn from(context: &CoreContext) -> Self {
        Self {
            bundle_identifier: context.bundle_identifier.clone(),
            core_generation: context.core_generation.clone(),
            store_generation: context.store_generation.clone(),
            identity: context.identity.clone(),
            started_at_ms: context.started_at_ms,
            resources_dir: context.resources_dir.clone(),
            claude: context.claude.clone(),
            discovery: context.discovery.clone(),
            received_ms: log::now_ms(),
        }
    }
}

/// Runs one discovery pass now, retrying final surface answers, and reports it.
fn refresh_evidence(context: &OpsContext) -> Result<ControlResponseBody, ControlError> {
    let (reply, answer) = mpsc::channel();
    context
        .discovery
        .send(Trigger::Refresh {
            force_surface: true,
            reply: Some(reply),
        })
        .map_err(|_| internal("discovery unavailable"))?;
    let summary = answer
        .recv_timeout(Duration::from_secs(30))
        .map_err(|_| internal("discovery did not answer"))?;
    Ok(ControlResponseBody::EvidenceRefreshed { summary })
}

fn internal(detail: impl Into<String>) -> ControlError {
    ControlError::new(ControlErrorCode::Internal, detail)
}

pub fn classify_automation(status: i32) -> AutomationPermission {
    match status {
        NO_ERR => AutomationPermission::Authorized,
        ERR_AE_EVENT_NOT_PERMITTED => AutomationPermission::Denied,
        ERR_AE_EVENT_WOULD_REQUIRE_USER_CONSENT => AutomationPermission::RequiresConsent,
        PROC_NOT_FOUND => AutomationPermission::TargetNotRunning,
        _ => AutomationPermission::Error,
    }
}

fn diagnostics(
    context: &OpsContext,
    writer: &SyncSender<WriterCommand>,
) -> Result<ControlResponseBody, ControlError> {
    let (reply, answer) = mpsc::channel();
    writer
        .send(WriterCommand::SqliteDiagnostics { reply })
        .map_err(|_| internal("writer unavailable"))?;
    let mut sqlite = answer
        .recv_timeout(QUICK)
        .map_err(|_| internal("writer did not answer"))?
        .map_err(internal)?;
    sqlite.database_path = redact_home(&sqlite.database_path);
    let mut process = context.identity.clone();
    process.executable_path = redact_home(&process.executable_path);
    Ok(ControlResponseBody::Diagnostics {
        report: CompanionDiagnostics {
            bundle_identifier: context.bundle_identifier.clone(),
            process,
            core_generation: context.core_generation.clone(),
            store_generation: context.store_generation.clone(),
            started_at_ms: context.started_at_ms,
            writer_lock_held: true,
            sqlite,
            notification_settings: bridge::notification_settings(QUICK),
            accessibility_preferences: bridge::accessibility_preferences(QUICK),
            qualification_build: cfg!(feature = "qualification"),
        },
    })
}

fn integration(context: &OpsContext) -> Result<ControlResponseBody, ControlError> {
    let app = PathBuf::from(TERMINAL_APP_PATH);
    let installed = app.exists();
    let running = bridge::application_running(TERMINAL_BUNDLE_ID, QUICK)
        .map_err(|error| internal(error.to_string()))?;
    let dictionary = if installed {
        terminal::probe_dictionary(&app)
            .map_err(|error| {
                log::warn(
                    "TERMINAL_DICTIONARY_FAILED",
                    json!({ "error": error.to_string() }),
                )
            })
            .ok()
    } else {
        None
    };
    // Asking without prompting; an unopened Terminal reports procNotFound.
    let status = if running {
        bridge::automation_permission(TERMINAL_BUNDLE_ID, false, QUICK)
            .map_err(|error| internal(error.to_string()))?
    } else {
        PROC_NOT_FOUND
    };
    let automation = classify_automation(status);
    let inventory = if running && automation == AutomationPermission::Authorized {
        let script = context.resources_dir.join("terminal-inventory.applescript");
        match terminal::enumerate(&script) {
            Ok(tabs) => Some(tabs.summary()),
            Err(error) => {
                log::warn(
                    "TERMINAL_INVENTORY_FAILED",
                    json!({ "error": error.to_string() }),
                );
                None
            }
        }
    } else {
        None
    };
    log::info(
        "INTEGRATION_STATUS",
        json!({
            "terminalRunning": running,
            "automation": automation,
            "automationStatus": status,
            "dictionarySha256": dictionary.as_ref().map(|dictionary| dictionary.sha256.clone()),
            "inventoryTabs": inventory.as_ref().map(|summary| summary.tab_count),
        }),
    );
    Ok(ControlResponseBody::IntegrationStatus {
        report: CompanionIntegration {
            notification_settings: bridge::notification_settings(QUICK),
            terminal: TerminalIntegration {
                application_path: installed.then(|| TERMINAL_APP_PATH.to_owned()),
                application_version: installed
                    .then(|| terminal::application_version(&app))
                    .flatten(),
                running,
                dictionary,
                automation,
                automation_status_code: status,
                inventory,
            },
        },
    })
}

fn request_notification_authorization() -> Result<ControlResponseBody, ControlError> {
    match bridge::call(
        |correlation_id| BridgeRequest::RequestNotificationAuthorization { correlation_id },
        PROMPT,
    ) {
        Ok(BridgeEvent::NotificationAuthorization {
            granted, settings, ..
        }) => {
            log::info(
                "NOTIFICATION_AUTHORIZATION_RESULT",
                json!({ "granted": granted, "authorizationStatus": settings.authorization_status, "alertSetting": settings.alert_setting }),
            );
            Ok(ControlResponseBody::NotificationAuthorization { granted, settings })
        }
        Ok(_) => Err(internal("unexpected bridge answer")),
        Err(error) => Err(internal(error.to_string())),
    }
}

fn request_terminal_automation() -> Result<ControlResponseBody, ControlError> {
    let running = bridge::application_running(TERMINAL_BUNDLE_ID, QUICK)
        .map_err(|error| internal(error.to_string()))?;
    let status = if running {
        bridge::automation_permission(TERMINAL_BUNDLE_ID, true, PROMPT)
            .map_err(|error| internal(error.to_string()))?
    } else {
        PROC_NOT_FOUND
    };
    let automation = classify_automation(status);
    log::info(
        "TERMINAL_AUTOMATION_RESULT",
        json!({ "automation": automation, "status": status }),
    );
    Ok(ControlResponseBody::TerminalAutomation {
        automation,
        status_code: status,
    })
}

pub fn run(
    body: ControlRequestBody,
    context: &OpsContext,
    writer: &SyncSender<WriterCommand>,
) -> Result<ControlResponseBody, ControlError> {
    match body {
        ControlRequestBody::Diagnostics => diagnostics(context, writer),
        ControlRequestBody::IntegrationStatus => integration(context),
        ControlRequestBody::RequestNotificationAuthorization => {
            request_notification_authorization()
        }
        ControlRequestBody::RequestTerminalAutomation => request_terminal_automation(),
        ControlRequestBody::ReturnToSession { route } => route::return_to_session(
            route,
            &context.claude,
            &context.discovery,
            context.received_ms,
        )
        .map(|result| ControlResponseBody::Routed {
            result: Box::new(result),
        }),
        ControlRequestBody::RefreshEvidence => refresh_evidence(context),
        _ => Err(ControlError::new(
            ControlErrorCode::BadRequest,
            "not a native operation",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_apple_event_statuses() {
        assert_eq!(classify_automation(0), AutomationPermission::Authorized);
        assert_eq!(classify_automation(-1743), AutomationPermission::Denied);
        assert_eq!(
            classify_automation(-1744),
            AutomationPermission::RequiresConsent
        );
        assert_eq!(
            classify_automation(-600),
            AutomationPermission::TargetNotRunning
        );
        assert_eq!(classify_automation(-50), AutomationPermission::Error);
    }
}
