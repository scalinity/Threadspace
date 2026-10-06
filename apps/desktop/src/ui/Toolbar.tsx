import type { ViewState } from "../bridge/client";

const PHASE_LABEL: Record<ViewState["phase"], string> = {
  connecting: "Connecting",
  hydrating: "Synchronizing",
  live: "Companion connected",
  unavailable: "Companion unavailable",
};

/**
 * The toolbar extends into the overlay titlebar. The whole bar is an explicit
 * drag region (`deep`); buttons and other controls inside it are excluded by
 * the framework, so they stay clickable.
 */
export function Toolbar({ state }: { state: ViewState }) {
  return (
    <header className="toolbar" data-tauri-drag-region="deep">
      <h1 className="wordmark">Threadspace</h1>
      <span className="toolbar__meta">M0A platform foundation</span>
      <span className={`pill pill--${state.phase}`} title={state.detail ?? undefined}>
        <span className="pill__dot" aria-hidden="true" />
        {PHASE_LABEL[state.phase]}
      </span>
      <span className="toolbar__counts" aria-label="Attention counts">
        <strong>{state.counts.needsAttention}</strong> need attention · <strong>{state.counts.awaitingAction}</strong> awaiting action
      </span>
    </header>
  );
}
