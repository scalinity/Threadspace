//! Qualification client: talks to a running companion through the same
//! verifying relay client the desktop uses, as the `QUALIFICATION` role that
//! only qualification builds accept. Output is one JSON document on stdout.
//!
//!   threadspace-qualify <dev|prod> diagnostics
//!   threadspace-qualify <dev|prod> integration
//!   threadspace-qualify <dev|prod> snapshot
//!   threadspace-qualify <dev|prod> raise-attention <label>
//!   threadspace-qualify <dev|prod> request-notifications
//!   threadspace-qualify <dev|prod> request-terminal

use std::process::ExitCode;
use std::time::Duration;

use serde_json::json;
use threadspace_contracts::control::{ClientRole, ControlRequestBody};
use threadspace_relay::client::{BlockingClient, connect};
use threadspace_relay::paths::{AgentPaths, agent_identifier_for};

fn usage() -> ExitCode {
    eprintln!(
        "usage: threadspace-qualify <dev|prod> <diagnostics|integration|snapshot|raise-attention LABEL|request-notifications|request-terminal>"
    );
    ExitCode::from(64)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let app_identifier = match args.first().map(String::as_str) {
        Some("dev") => "ai.scalinity.threadspace.dev",
        Some("prod") => "ai.scalinity.threadspace",
        _ => return usage(),
    };
    let Some(command) = args.get(1) else {
        return usage();
    };
    let Some(paths) = AgentPaths::for_agent(&agent_identifier_for(app_identifier)) else {
        return usage();
    };

    let connection = match connect(
        &paths.locator,
        ClientRole::Qualification,
        Duration::from_secs(3),
    ) {
        Ok(connection) => connection,
        Err(error) => {
            println!(
                "{}",
                json!({ "ok": false, "stage": "connect", "error": error.to_string() })
            );
            return ExitCode::from(2);
        }
    };
    let peer = json!({
        "pid": connection.peer.pid,
        "coreGeneration": connection.hello.core_generation,
        "storeGeneration": connection.hello.store_generation,
        "companion": connection.hello.companion,
    });
    let mut client = BlockingClient::new(connection);

    let request = match command.as_str() {
        "diagnostics" => ControlRequestBody::Diagnostics,
        "integration" => ControlRequestBody::IntegrationStatus,
        "snapshot" => ControlRequestBody::AttachView {
            subscription_id: uuid::Uuid::new_v4().to_string(),
        },
        "raise-attention" => ControlRequestBody::QualifyRaiseAttention {
            label: args
                .get(2)
                .cloned()
                .unwrap_or_else(|| "qualification".into()),
        },
        "request-notifications" => ControlRequestBody::RequestNotificationAuthorization,
        "request-terminal" => ControlRequestBody::RequestTerminalAutomation,
        _ => return usage(),
    };
    if matches!(
        request,
        ControlRequestBody::RequestNotificationAuthorization
            | ControlRequestBody::RequestTerminalAutomation
    ) {
        // Waits for the owner to answer a native prompt.
        let _ = client.set_read_timeout(Duration::from_secs(200));
    }
    let detach = match &request {
        ControlRequestBody::AttachView { subscription_id } => Some(subscription_id.clone()),
        _ => None,
    };
    let outcome = client.request(request);
    if let Some(subscription_id) = detach {
        let _ = client.request(ControlRequestBody::DetachView { subscription_id });
    }
    match outcome {
        Ok(body) => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({ "ok": true, "peer": peer, "response": body })
                )
                .unwrap_or_default()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            println!(
                "{}",
                json!({ "ok": false, "peer": peer, "error": error.to_string() })
            );
            ExitCode::from(1)
        }
    }
}
