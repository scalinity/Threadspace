// Launch configuration injected natively by an initialization script
// (`window.__THREADSPACE_LAUNCH__`). Outside Tauri it falls back to defaults.

export interface LaunchConfig {
  rendererMode: "webgpu" | "webgl2-compatibility";
  qualificationBuild: boolean;
  qualifyIpc: boolean;
  probe: "acl" | null;
}

function read(): LaunchConfig {
  const raw = (window as unknown as { __THREADSPACE_LAUNCH__?: Record<string, unknown> }).__THREADSPACE_LAUNCH__;
  return {
    rendererMode: raw?.rendererMode === "webgl2-compatibility" ? "webgl2-compatibility" : "webgpu",
    qualificationBuild: raw?.qualificationBuild === true,
    qualifyIpc: raw?.qualifyIpc === true,
    probe: raw?.probe === "acl" ? "acl" : null,
  };
}

export const launch: LaunchConfig = read();
