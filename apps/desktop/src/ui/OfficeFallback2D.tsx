import type { RendererLifecycleSnapshot } from "@threadspace/scene";

import type { BridgeClient, ViewState } from "../bridge/client";
import { openItemsFor } from "./attention";
import { WORKER_STATE_TEXT, sceneModel } from "./worker";

/**
 * The DOM office: the same workers and attention the 3D scene draws, from the
 * same view model. Shown in 2D mode and after the renderer falls back; it
 * never touches journal or attention state.
 */
export function OfficeFallback2D({ state, client, renderer }: { state: ViewState; client: BridgeClient; renderer: RendererLifecycleSnapshot }) {
  const model = sceneModel(state);
  return (
    <section className="office-2d" aria-labelledby="office-2d-heading">
      <h2 id="office-2d-heading" className="office-2d__title">Office</h2>
      {renderer.presentation === "fallback" ? (
        <p className="callout" role="status">
          The 3D office stopped: {renderer.fallbackReason}. Sessions, attention and Return keep working here. Turn off 2D view to try 3D again.
        </p>
      ) : (
        <p className="panel__subtitle">2D view. The 3D renderer is released while this view is open.</p>
      )}
      <ul className="list">
        {model.workers.map((worker) => {
          const selected = worker.id === model.selectedId;
          const open = openItemsFor(state.attention, worker.id);
          const categories = [...new Set(open.map((item) => item.category.toLowerCase().replaceAll("_", " ")))];
          return (
            <li key={worker.id} className={`row ${selected ? "row--selected" : ""} ${worker.state === "ended" ? "row--ended" : ""}`}>
              <button type="button" className="row__main" onClick={() => client.select(worker.id)} aria-current={selected ? "true" : undefined}>
                <span className="row__label">
                  <span className={`office-2d__marker office-2d__marker--${worker.state}`} aria-hidden="true" />
                  {worker.label}
                </span>
                <span className="row__meta">
                  {WORKER_STATE_TEXT[worker.state]}
                  {open.length > 0 ? ` · ${open.length} open · ${categories.join(", ")}` : ""}
                </span>
              </button>
            </li>
          );
        })}
        {model.workers.length === 0 ? <li className="empty">No sessions in the projection.</li> : null}
      </ul>
    </section>
  );
}
