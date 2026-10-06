# Failed attempt: the sustained phase captured a retired window

Build `8c82212`. Ten of eleven checks passed, including per-minute
DOM/journal agreement for minutes 7 to 15. Every live phase before the
document reload changed in the canvas; the sustained phase after it had 0
of 95 changed pairs, and its frames show an empty window (`frames/0088-sustained.png`).

The reload recovered the office view correctly (`OFFICE_VIEW_RECOVERED`,
`MAIN_DOCUMENT_REPLACED`, removed and created) and the new window rendered,
but the retired window stayed registered with the window server off screen
at the same size. The harness picked the main window by area alone and the
two tied, so it captured the retired one. A reproduction counted one
lingering off-screen window per recovery (6 recoveries: +6 windows, +0.2 MB
physical footprint) while the WebContent process count stayed at 1: the
web views and their content processes are released; only the empty native
window shell remains after Tauri reports removal. The harness now prefers
an on-screen window, and view recovery records the window shells; the shell
leak is recorded as an open risk in the M0C failure ledger.
