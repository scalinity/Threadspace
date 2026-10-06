import { useSyncExternalStore } from "react";

import type { RendererLifecycleSnapshot } from "@threadspace/scene";

import type { BridgeClient, ViewState } from "../bridge/client";
import { AttentionPanel } from "./AttentionPanel";
import { DiagnosticsPanel } from "./DiagnosticsPanel";
import { FleetPanel } from "./FleetPanel";
import { Inspector } from "./Inspector";
import { OfficeFallback2D } from "./OfficeFallback2D";
import { SceneView, rendererLifecycleFor } from "./SceneView";
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
          <OfficeStage state={state} client={client} />
        </section>
        <aside className="detail" aria-label="Inspector and diagnostics">
          <Inspector state={state} client={client} />
          <DiagnosticsPanel state={state} client={client} />
        </aside>
      </main>
    </div>
  );
}

function OfficeStage({ state, client }: { state: ViewState; client: BridgeClient }) {
  const lifecycle = rendererLifecycleFor(client);
  const renderer = useSyncExternalStore(lifecycle.subscribe, lifecycle.getSnapshot);
  const flat = renderer.presentation !== "3d";
  return (
    <>
      <SceneView client={client} lifecycle={lifecycle} hidden={flat} />
      {flat ? <OfficeFallback2D state={state} client={client} renderer={renderer} /> : null}
      <div className="stage-controls">
        <button type="button" className="button button--quiet stage-toggle" aria-pressed={flat} onClick={() => lifecycle.setMode2d(!flat)}>
          2D view
        </button>
      </div>
      <RendererBadge renderer={renderer} />
    </>
  );
}

function lossSummary(renderer: RendererLifecycleSnapshot): string {
  const { natural, injected } = renderer.lossCounts;
  if (natural + injected === 0) return "";
  return ` · GPU loss ${natural} natural, ${injected} injected`;
}

function RendererBadge({ renderer }: { renderer: RendererLifecycleSnapshot }) {
  const generation = renderer.generation > 0 ? ` · gen ${renderer.generation}` : "";
  const losses = lossSummary(renderer);
  switch (renderer.state) {
    case "failed-fallback":
      return <div className="renderer-badge renderer-badge--fail" role="status">3D unavailable — {renderer.fallbackReason}{losses}</div>;
    case "2d":
      return <div className="renderer-badge" role="status">2D view · renderer released{generation}{losses}</div>;
    case "hidden-disposed":
      return <div className="renderer-badge" role="status">Hidden · renderer released{generation}{losses}</div>;
    case "detached":
    case "initializing":
      return <div className="renderer-badge" role="status">Initializing renderer…{generation}</div>;
    case "rebuilding":
    case "disposing":
      return <div className="renderer-badge" role="status">Rebuilding renderer…{generation}{losses}</div>;
    case "live":
      break;
  }
  const attestation = renderer.lastAttestation;
  if (attestation?.backend === "WEBGPU" && attestation.webgpuGate === "PASS") {
    return <div className="renderer-badge renderer-badge--ok" role="status">WebGPU · three r{attestation.threeRevision}{generation}{losses}</div>;
  }
  return (
    <div className="renderer-badge renderer-badge--fail" role="status">
      {attestation?.backend ?? "UNVERIFIED"} — diagnostic backend; does not pass the WebGPU gate{generation}{losses}
    </div>
  );
}
