import type { BridgeClient, ViewState } from "../bridge/client";

function Row({ label, value }: { label: string; value: string | number | boolean | null | undefined }) {
  return (
    <div className="field">
      <dt>{label}</dt>
      <dd className="mono">{value === null || value === undefined ? "—" : String(value)}</dd>
    </div>
  );
}

export function DiagnosticsPanel({ state, client }: { state: ViewState; client: BridgeClient }) {
  const desktop = state.diagnostics?.desktop;
  const companion = state.diagnostics?.companion;
  const terminal = state.integration?.companion?.terminal;
  const notifications = companion?.notificationSettings ?? state.integration?.companion?.notificationSettings;
  const renderer = state.renderer;
  return (
    <section className="panel" aria-labelledby="diagnostics-heading">
      <h2 id="diagnostics-heading" className="panel__title">Diagnostics</h2>
      <div className="actions">
        <button type="button" className="button button--quiet" onClick={() => void client.refreshDiagnostics()} disabled={state.phase !== "live"}>
          Refresh
        </button>
        <button type="button" className="button button--quiet" onClick={() => void client.requestSetup("RequestNotificationAuthorization")} disabled={state.phase !== "live"}>
          Allow notifications
        </button>
        <button type="button" className="button button--quiet" onClick={() => void client.setObservation(!(companion?.observationEnabled ?? false))}>
          {companion?.observationEnabled === false || companion === undefined || companion === null ? "Enable observation" : "Stop observation"}
        </button>
        <button type="button" className="button button--quiet" onClick={() => void client.requestSetup("RequestTerminalAutomation")} disabled={state.phase !== "live"}>
          Allow Terminal access
        </button>
      </div>
      <h3 className="section-title">Renderer</h3>
      <dl className="fields">
        <Row label="Class" value={renderer?.rendererClass} />
        <Row label="Backend" value={renderer?.backend} />
        <Row label="WebGPU gate" value={renderer?.webgpuGate} />
        <Row label="Three" value={renderer ? `r${renderer.threeRevision}` : null} />
        <Row label="navigator.gpu" value={renderer?.navigatorGpu} />
        <Row label="Secure context" value={renderer?.secureContext} />
        <Row label="Origin" value={renderer?.origin} />
        <Row label="Adapter" value={renderer?.adapter ? `${renderer.adapter.vendor} ${renderer.adapter.architecture}`.trim() || "reported" : null} />
      </dl>
      <h3 className="section-title">Bridge</h3>
      <dl className="fields">
        <Row label="Phase" value={state.phase} />
        <Row label="Cursor" value={state.cursor} />
        <Row label="Stream seq" value={state.stream.lastSeq} />
        <Row label="Patches applied" value={state.stream.patchesApplied} />
        <Row label="Core generation" value={state.connection?.coreGeneration.slice(0, 8)} />
        <Row label="Store generation" value={state.connection?.storeGeneration.slice(0, 8)} />
      </dl>
      <h3 className="section-title">Companion</h3>
      <dl className="fields">
        <Row label="Bundle" value={companion?.bundleIdentifier} />
        <Row label="PID" value={companion?.process.pid} />
        <Row label="SQLite" value={companion ? `${companion.sqlite.version} · ${companion.sqlite.journalMode} · sync ${companion.sqlite.synchronous}` : null} />
        <Row label="Notifications" value={notifications ? `${notifications.authorizationStatus} · alert ${notifications.alertSetting}` : null} />
        <Row label="Reduce motion" value={companion?.accessibilityPreferences?.reduceMotion} />
        <Row label="Observation" value={companion ? (companion.observationEnabled ? "enabled" : "stopped") : null} />
        <Row label="Maintenance" value={companion?.maintenancePhase} />
        <Row label="Sleep / wake" value={companion ? `${companion.power.sleeps} / ${companion.power.wakes}` : null} />
        <Row label="Login item" value={state.integration?.service.status} />
        <Row label="Terminal" value={terminal ? `${terminal.applicationVersion ?? "?"} · ${terminal.automation} · ${terminal.inventory ? `${terminal.inventory.tabCount} tabs` : "no inventory"}` : null} />
      </dl>
      <h3 className="section-title">Shell</h3>
      <dl className="fields">
        <Row label="App" value={desktop ? `${desktop.appIdentifier} ${desktop.appVersion}` : null} />
        <Row label="Tauri" value={desktop?.tauriVersion} />
        <Row label="macOS" value={desktop ? `${desktop.osProductVersion} (${desktop.osBuildVersion})` : null} />
        <Row label="WebKit" value={desktop?.webkitVersion} />
      </dl>
      {state.notices.length > 0 ? (
        <ol className="notices" aria-label="Recent bridge events">
          {state.notices.map((notice) => (
            <li key={notice}>{notice}</li>
          ))}
        </ol>
      ) : null}
    </section>
  );
}
