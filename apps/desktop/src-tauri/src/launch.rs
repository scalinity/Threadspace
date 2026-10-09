//! Launch arguments. The renderer receives its launch configuration through a
//! native initialization script, so production URLs never carry flags.

use std::path::PathBuf;

use serde_json::json;
use threadspace_provider_claude::setup::Scope;

use crate::bootstrap::ServiceCommand;
use crate::integration::{self, IntegrationAction, IntegrationCommand};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RendererMode {
    /// The selected WebGPU backend.
    #[default]
    WebGpu,
    /// Diagnostic run forcing the renderer's WebGL2 backend. It is visibly
    /// labelled `WEBGL2_COMPATIBILITY` and can never pass the WebGPU gate.
    Webgl2Compatibility,
}

#[derive(Debug, Clone, Default)]
pub struct LaunchOptions {
    pub service: Option<ServiceCommand>,
    pub integration: Option<IntegrationCommand>,
    pub renderer: RendererMode,
    /// Qualification builds only: run the in-app IPC self-test after hydration.
    pub qualify_ipc: bool,
    /// Qualification builds only: open an unauthorized second view to prove
    /// the capability refuses it.
    pub qualify_acl_probe: bool,
    /// Qualification builds only: open a view on this loopback HTTP URL to
    /// prove a remote origin cannot reach the bridge commands.
    pub qualify_origin_probe: Option<String>,
}

impl LaunchOptions {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let mut options = Self::default();
        let mut integration = None;
        let mut config_dir = None;
        // An unrecognized scope cancels the command instead of falling back
        // to editing the user settings.
        let mut scope = Some(Scope::User);
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--service" => {
                    options.service = args.next().as_deref().and_then(ServiceCommand::parse)
                }
                "--integration" => {
                    integration = args.next().as_deref().and_then(IntegrationAction::parse)
                }
                "--config-dir" => config_dir = args.next().map(PathBuf::from),
                "--scope" => scope = args.next().as_deref().and_then(integration::parse_scope),
                "--renderer=webgl2-compatibility" => {
                    options.renderer = RendererMode::Webgl2Compatibility
                }
                "--qualify-ipc" if cfg!(feature = "qualification") => options.qualify_ipc = true,
                "--qualify-acl-probe" if cfg!(feature = "qualification") => {
                    options.qualify_acl_probe = true
                }
                probe
                    if cfg!(feature = "qualification")
                        && probe.starts_with("--qualify-origin-probe=") =>
                {
                    let url = &probe["--qualify-origin-probe=".len()..];
                    // Loopback only: the probe never loads anything off-machine.
                    if url.starts_with("http://127.0.0.1:") {
                        options.qualify_origin_probe = Some(url.to_owned());
                    }
                }
                // Unknown arguments (for example Finder's legacy -psn_*) are ignored.
                _ => {}
            }
        }
        options.integration = integration
            .zip(scope)
            .map(|(action, scope)| IntegrationCommand {
                action,
                config_dir,
                scope,
            });
        options
    }

    pub fn initialization_script(&self, probe: Option<&str>) -> String {
        let config = json!({
            "rendererMode": match self.renderer {
                RendererMode::WebGpu => "webgpu",
                RendererMode::Webgl2Compatibility => "webgl2-compatibility",
            },
            "qualificationBuild": cfg!(feature = "qualification"),
            "qualifyIpc": self.qualify_ipc,
            "probe": probe,
        });
        format!(
            "Object.defineProperty(window, '__THREADSPACE_LAUNCH__', {{ value: Object.freeze({config}), writable: false, configurable: false }});"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_service_and_renderer_flags() {
        let options =
            LaunchOptions::parse(["--service".into(), "status".into(), "-psn_0_1".into()]);
        assert_eq!(options.service, Some(ServiceCommand::Status));
        let options = LaunchOptions::parse(["--renderer=webgl2-compatibility".into()]);
        assert_eq!(options.renderer, RendererMode::Webgl2Compatibility);
        assert!(
            options
                .initialization_script(None)
                .contains("webgl2-compatibility")
        );
    }

    #[test]
    fn parses_integration_flags() {
        let parse = |args: &[&str]| {
            LaunchOptions::parse(args.iter().map(|arg| (*arg).to_owned())).integration
        };
        assert_eq!(
            parse(&["--integration", "status"]),
            Some(IntegrationCommand {
                action: IntegrationAction::Status,
                config_dir: None,
                scope: Scope::User,
            })
        );
        assert_eq!(
            parse(&[
                "--scope",
                "session",
                "--integration",
                "install",
                "--config-dir",
                "/tmp/c"
            ]),
            Some(IntegrationCommand {
                action: IntegrationAction::Install,
                config_dir: Some(PathBuf::from("/tmp/c")),
                scope: Scope::Session,
            })
        );
        assert_eq!(
            parse(&["--integration", "install", "--scope", "sesion"]),
            None
        );
        assert_eq!(parse(&["--integration", "reinstall"]), None);
        assert_eq!(parse(&["--scope", "user"]), None);
    }
}
