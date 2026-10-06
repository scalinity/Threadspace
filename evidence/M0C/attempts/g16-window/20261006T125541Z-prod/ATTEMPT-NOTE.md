# Smoke attempt: plain Tab, and a one-shot Reduce Motion read

Build `a983837`. Fourteen checks ran: `dpr-change` passed (the drawing
buffer followed 2x -> 1x -> 2x with the scene live and the display
restored), `accessibility-tree` passed, `display-disconnect` is
MANUAL_EXTERNAL_REQUIRED.

- `keyboard-navigation`: plain Tab left focus on the window. macOS Keyboard
  navigation is off (`AppleKeyboardUIMode` 0, the default), so Tab reaches
  only text fields and lists in WebKit, as in native apps. The walk now uses
  Option-Tab, which reaches every control, and records the mode.
- `reduce-motion`: the switch toggled on and was restored off, and the
  companion read it, but one renderer read three seconds later returned
  false. A separate probe on the same build saw the renderer report true on
  its first poll after the toggle. The check now polls the renderer for up
  to ten seconds, and proves the canvas moves before the toggle and holds
  still under Reduce Motion with the centre workers in the attention state.
