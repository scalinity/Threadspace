//! Launch arguments. The renderer receives its launch configuration through a
//! native initialization script, so production URLs never carry flags.

use serde_json::json;

use crate::bootstrap::ServiceCommand;

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
    pub renderer: RendererMode,
    /// Qualification builds only: run the in-app IPC self-test after hydration.
    pub qualify_ipc: bool,
    /// Qualification builds only: open an unauthorized second view to prove
    /// the capability refuses it.
    pub qualify_acl_probe: bool,
}

impl LaunchOptions {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let mut options = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--service" => {
                    options.service = args.next().as_deref().and_then(ServiceCommand::parse)
                }
                "--renderer=webgl2-compatibility" => {
                    options.renderer = RendererMode::Webgl2Compatibility
                }
                "--qualify-ipc" if cfg!(feature = "qualification") => options.qualify_ipc = true,
                "--qualify-acl-probe" if cfg!(feature = "qualification") => {
                    options.qualify_acl_probe = true
                }
                // Unknown arguments (for example Finder's legacy -psn_*) are ignored.
                _ => {}
            }
        }
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
}
