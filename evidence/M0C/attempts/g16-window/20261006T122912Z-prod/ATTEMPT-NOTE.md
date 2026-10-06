# Failed attempt: harness reads and an untested display cell

Build `a6ef67a`, harness `8e5481c`. Ten checks passed (traffic lights,
focus, content and titlebar drag, minimize, restore focus, zoom, fullscreen,
bounds across relaunch, off-screen correction).

- `keyboard-navigation`: every focus read returned the window. WebKit builds
  its web accessibility tree on the first request; the run never warmed it.
- `accessibility-tree`: the three unlabelled buttons are the window's own
  close, minimize and full-screen buttons (`AXCloseButton`,
  `AXMinimizeButton`, `AXFullScreenButton`), which macOS names from their
  subroles; all 125 content buttons were labelled.
- `reduce-motion`: on macOS 27 the switch is under Accessibility > Motion,
  not Display, and it is an unlabelled `AXSwitch` directly after the static
  text "Reduce motion"; the switch finder matched only a switch's own title.
- `dpr-change`: recorded NOT_RUN on the claim that every mode of the
  built-in panel is 2x. The panel offers 60 usable 1x modes once
  low-resolution duplicates are listed; the claim was never measured. The
  check is now real (an app-scoped 1x mode, restored by macOS when the
  helper exits; a SIGKILL restore was verified), and it exposed that the
  renderer never followed device-pixel-ratio changes, fixed in 7e8043d.
  Display disconnect is recorded MANUAL_EXTERNAL_REQUIRED.
