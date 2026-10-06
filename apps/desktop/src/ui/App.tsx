import { useSyncExternalStore } from "react";

import type { BridgeClient } from "../bridge/client";
import { AttentionPanel } from "./AttentionPanel";
import { DiagnosticsPanel } from "./DiagnosticsPanel";
import { FleetPanel } from "./FleetPanel";
import { Inspector } from "./Inspector";
import { SceneView } from "./SceneView";
import { Toolbar } from "./Toolbar";

export function App({ client }: { client: BridgeClient }) {
  const state = useSyncExternalStore(client.subscribe, client.getSnapshot);
  return (
    <div className="shell">
      <Toolbar state={state} />
      <main className="workspace">
        <aside className="rail" aria-label="Attention and fleet">
          <AttentionPanel state={state} client={client} />
          <FleetPanel state={state} client={client} />
        </aside>
        <section className="stage" aria-label="Office">
          <SceneView client={client} />
          <RendererBadge state={state} />
        </section>
        <aside className="detail" aria-label="Inspector and diagnostics">
          <Inspector state={state} client={client} />
          <DiagnosticsPanel state={state} client={client} />
        </aside>
      </main>
    </div>
  );
}

function RendererBadge({ state }: { state: ReturnType<BridgeClient["getSnapshot"]> }) {
  if (state.rendererError) {
    return <div className="renderer-badge renderer-badge--fail" role="status">Renderer unavailable — {state.rendererError}</div>;
  }
  const renderer = state.renderer;
  if (!renderer) return <div className="renderer-badge" role="status">Initializing renderer…</div>;
  if (renderer.backend === "WEBGPU" && renderer.webgpuGate === "PASS") {
    return <div className="renderer-badge renderer-badge--ok" role="status">WebGPU · three r{renderer.threeRevision}</div>;
  }
  return (
    <div className="renderer-badge renderer-badge--fail" role="status">
      {renderer.backend} — diagnostic backend; does not pass the WebGPU gate
    </div>
  );
}
