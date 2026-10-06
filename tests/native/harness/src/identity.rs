//! The two installed qualification identities and every path the harness
//! reads. Production (`Threadspace.app`) carries the packaged acceptance
//! runs; development (`Threadspace Dev.app`) carries store-polluting fixtures
//! such as oversized snapshots, so the production store stays clean.

use std::path::PathBuf;

use threadspace_relay::paths::{AgentPaths, agent_identifier_for, home_dir};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Prod,
    Dev,
}

impl Channel {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "prod" => Some(Self::Prod),
            "dev" => Some(Self::Dev),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Identity {
    pub channel: Channel,
    pub app_identifier: String,
    pub agent_identifier: String,
    pub bundle: PathBuf,
    pub executable: PathBuf,
    pub companion_executable: PathBuf,
    pub agent: AgentPaths,
    /// `~/Library/Logs/<app id>` — Tauri's app log directory.
    pub app_log_dir: PathBuf,
}

impl Identity {
    pub fn installed(channel: Channel) -> Option<Self> {
        let (app_identifier, bundle_name) = match channel {
            Channel::Prod => ("ai.scalinity.threadspace", "Threadspace.app"),
            Channel::Dev => ("ai.scalinity.threadspace.dev", "Threadspace Dev.app"),
        };
        let home = home_dir()?;
        let bundle = home.join("Applications").join(bundle_name);
        let main = match channel {
            Channel::Prod => "Threadspace",
            Channel::Dev => "Threadspace Dev",
        };
        let agent_identifier = agent_identifier_for(app_identifier);
        Some(Self {
            channel,
            app_identifier: app_identifier.to_owned(),
            agent: AgentPaths::for_agent(&agent_identifier)?,
            agent_identifier,
            executable: bundle.join("Contents/MacOS").join(main),
            companion_executable: bundle.join(
                "Contents/Library/LoginItems/ThreadspaceAgent.app/Contents/MacOS/ThreadspaceAgent",
            ),
            app_log_dir: home.join("Library/Logs").join(app_identifier),
            bundle,
        })
    }

    pub fn reports_dir(&self) -> PathBuf {
        self.app_log_dir.join("qualification")
    }

    pub fn desktop_log(&self) -> PathBuf {
        self.app_log_dir.join("desktop.log")
    }

    pub fn companion_log(&self) -> PathBuf {
        self.agent.log_dir.join("agent.log")
    }

    /// Where the desktop shell keeps its window bounds (Tauri app config dir).
    pub fn bounds_file(&self) -> Option<PathBuf> {
        Some(
            home_dir()?
                .join("Library/Application Support")
                .join(&self.app_identifier)
                .join("window-bounds.json"),
        )
    }
}
