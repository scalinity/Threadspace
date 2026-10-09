//! The Claude integration setup command (SPEC §19.2). Like `--service`, it
//! runs without constructing React or a WebView and prints one JSON line:
//!
//!   Threadspace.app/Contents/MacOS/Threadspace --integration plan|install|uninstall|status
//!       [--config-dir <dir>] [--scope user|session]
//!
//! The configuration directory defaults to `~/.claude` and the scope to user.
//! The helper and the observer mod are staged from the nested companion
//! bundle into `~/Library/Application Support/<app id>/integrations/claude`.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use threadspace_provider_claude::setup::{self, Scope, Target};
use threadspace_relay::paths::{agent_identifier_for, home_dir};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationAction {
    Plan,
    Install,
    Uninstall,
    Status,
}

impl IntegrationAction {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "plan" => Some(Self::Plan),
            "install" => Some(Self::Install),
            "uninstall" => Some(Self::Uninstall),
            "status" => Some(Self::Status),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationCommand {
    pub action: IntegrationAction,
    pub config_dir: Option<PathBuf>,
    pub scope: Scope,
}

pub fn parse_scope(value: &str) -> Option<Scope> {
    match value {
        "user" => Some(Scope::User),
        "session" => Some(Scope::Session),
        _ => None,
    }
}

pub fn run_cli(command: &IntegrationCommand, app_identifier: &str) -> i32 {
    let result = target(command, app_identifier).and_then(|target| {
        let detail = match command.action {
            IntegrationAction::Plan => setup::plan(&target).map(|plan| json!(plan)),
            IntegrationAction::Install => setup::install(&target).map(|record| json!(record)),
            IntegrationAction::Uninstall => setup::uninstall(&target).map(|report| json!(report)),
            IntegrationAction::Status => setup::status(&target).map(|state| json!(state)),
        };
        detail.map_err(|error| json!({ "error": error.to_string(), "setupError": error }))
    });
    let (ok, detail) = match result {
        Ok(detail) => (true, detail),
        Err(detail) => (false, detail),
    };
    println!(
        "{}",
        json!({
            "operation": format!("{:?}", command.action).to_lowercase(),
            "appIdentifier": app_identifier,
            "scope": command.scope,
            "ok": ok,
            "detail": detail,
        })
    );
    i32::from(!ok)
}

fn target(command: &IntegrationCommand, app_identifier: &str) -> Result<Target, Value> {
    let failed = |message: String| json!({ "error": message });
    let home = home_dir().ok_or_else(|| failed("the home directory is unavailable".into()))?;
    let config_dir = match &command.config_dir {
        // Absolute, so the install record still names it from any directory.
        Some(dir) => std::path::absolute(dir).map_err(|error| failed(error.to_string()))?,
        None => home.join(".claude"),
    };
    // Threadspace.app/Contents/MacOS/Threadspace → Threadspace.app/Contents
    let contents = std::env::current_exe()
        .ok()
        .and_then(|executable| executable.parent()?.parent().map(Path::to_owned))
        .ok_or_else(|| failed("the application bundle cannot be located".into()))?;
    let companion = contents.join("Library/LoginItems/ThreadspaceAgent.app/Contents");
    Ok(Target {
        config_dir,
        owned_dir: home
            .join("Library/Application Support")
            .join(app_identifier)
            .join("integrations/claude"),
        agent_identifier: agent_identifier_for(app_identifier),
        helper_source: companion.join("MacOS/threadspace-hook"),
        mod_source: companion.join("Resources/provider-mod"),
        scope: command.scope,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_the_owned_dir_and_bundled_sources() {
        let command = IntegrationCommand {
            action: IntegrationAction::Plan,
            config_dir: Some(PathBuf::from("relative/config")),
            scope: Scope::Session,
        };
        let resolved = target(&command, "ai.example.threadspace").expect("target");
        let home = home_dir().expect("home");
        assert_eq!(
            resolved.owned_dir,
            home.join("Library/Application Support/ai.example.threadspace/integrations/claude")
        );
        assert_eq!(resolved.agent_identifier, "ai.example.threadspace.agent");
        assert_eq!(resolved.scope, Scope::Session);
        assert!(
            resolved.config_dir.is_absolute() && resolved.config_dir.ends_with("relative/config")
        );
        let companion = Path::new("Library/LoginItems/ThreadspaceAgent.app/Contents");
        assert!(
            resolved
                .helper_source
                .ends_with(companion.join("MacOS/threadspace-hook"))
        );
        assert!(
            resolved
                .mod_source
                .ends_with(companion.join("Resources/provider-mod"))
        );

        let defaults = IntegrationCommand {
            config_dir: None,
            ..command
        };
        let resolved = target(&defaults, "ai.example.threadspace").expect("target");
        assert_eq!(resolved.config_dir, home.join(".claude"));
    }
}
