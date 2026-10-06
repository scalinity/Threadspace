//! The outer application's native bootstrap (SPEC §18.9): it alone registers,
//! queries and unregisters the single `SMAppService.loginItem(identifier:)` for
//! the nested `ThreadspaceAgent.app`. It runs without constructing React or a
//! WebView:
//!
//!   Threadspace.app/Contents/MacOS/Threadspace --service status|register|unregister
//!
//! No custom LaunchAgent is registered alongside it.

use objc2::rc::Retained;
use objc2_foundation::NSString;
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use serde_json::json;
use threadspace_contracts::diagnostics::{ServiceReport, ServiceStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceCommand {
    Status,
    Register,
    Unregister,
}

impl ServiceCommand {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "status" => Some(Self::Status),
            "register" => Some(Self::Register),
            "unregister" => Some(Self::Unregister),
            _ => None,
        }
    }
}

fn service(agent_identifier: &str) -> Retained<SMAppService> {
    let identifier = NSString::from_str(agent_identifier);
    // SAFETY: a plain class-method call with a valid NSString argument.
    unsafe { SMAppService::loginItemServiceWithIdentifier(&identifier) }
}

fn status_of(service: &SMAppService) -> ServiceStatus {
    // SAFETY: reading a property of a live SMAppService.
    let status = unsafe { service.status() };
    match status {
        SMAppServiceStatus::NotRegistered => ServiceStatus::NotRegistered,
        SMAppServiceStatus::Enabled => ServiceStatus::Enabled,
        SMAppServiceStatus::RequiresApproval => ServiceStatus::RequiresApproval,
        SMAppServiceStatus::NotFound => ServiceStatus::NotFound,
        _ => ServiceStatus::Unknown,
    }
}

pub fn report(agent_identifier: &str) -> ServiceReport {
    ServiceReport { agent_identifier: agent_identifier.to_owned(), status: status_of(&service(agent_identifier)) }
}

/// Runs one bootstrap command and prints a JSON result line. Exit 0 on success.
pub fn run_cli(command: ServiceCommand, app_identifier: &str) -> i32 {
    let agent_identifier = threadspace_relay::paths::agent_identifier_for(app_identifier);
    let service = service(&agent_identifier);
    let before = status_of(&service);
    // SAFETY: plain method calls on a live SMAppService; errors are returned.
    let outcome = match command {
        ServiceCommand::Status => Ok(()),
        ServiceCommand::Register => unsafe { service.registerAndReturnError() },
        ServiceCommand::Unregister => unsafe { service.unregisterAndReturnError() },
    };
    let error = outcome.err().map(|error| {
        format!("{} {} {}", error.domain(), error.code(), error.localizedDescription())
    });
    let after = status_of(&service);
    println!(
        "{}",
        json!({
            "operation": format!("{command:?}").to_lowercase(),
            "appIdentifier": app_identifier,
            "agentIdentifier": agent_identifier,
            "statusBefore": before,
            "statusAfter": after,
            "error": error,
        })
    );
    i32::from(error.is_some())
}
