# D-0002 — Grant the two core window permissions that titlebar drag regions use

**Status:** Accepted; ratified during the independent M0C review on 2026-10-06 using inherited ACL evidence and native G16 drag/zoom qualification. This does not accept M0C as a whole.
**Affects:** SPEC §18.6 (the `office-local` capability example lists only the five app-command permissions), §15.5 and §18.8 (overlay titlebar with explicit drag regions), MILESTONES G16.

## Context

SPEC §18.8 selects an opaque content surface with a native overlay titlebar, standard traffic lights and "draggable regions explicit[,] exclud[ing] buttons/inputs". With an overlay titlebar the WKWebView covers the titlebar strip, so the window moves only through web-declared drag regions.

In the pinned `tauri 3.0.0-alpha.4` source, `src/window/scripts/drag.js` implements `data-tauri-drag-region` by invoking `plugin:window|start_dragging` on mousedown, and `plugin:window|internal_toggle_maximize` on double-click (on mouseup for macOS). `src/webview/mod.rs` (`Webview::on_message`) checks the ACL for every `plugin:` command; only the internal Channel fetch command is exempt. With the capability exactly as printed in SPEC §18.6 (five `allow-ui-*` permissions), both calls are refused and the window cannot be dragged. That would fail G16.

## Decision

Add exactly two permissions to `office-local`:

- `core:window:allow-start-dragging`
- `core:window:allow-internal-toggle-maximize`

The capability stays scoped to the `office` webview, local origin only and macOS only. No core default set, window-management permission or other plugin command is granted. SPEC §18.6's "no … broad core default permission is needed **for these five app commands**" still holds: these two permissions serve the titlebar, not the app commands.

## Consequences

- Drag regions and native double-click zoom work through the framework's public mechanism, with no private API or native overlay view.
- The renderer can start a window drag or toggle zoom. It cannot close, move programmatically, resize, or open windows, because those permissions are absent. The M0A IPC self-test verifies that `plugin:window|close` is refused.
- SPEC §18.6 now includes these exact two titlebar permissions; the application-command and local-view boundaries remain unchanged.
