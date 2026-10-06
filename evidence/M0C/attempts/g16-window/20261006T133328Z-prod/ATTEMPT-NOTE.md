# Failed attempt: fullscreen and zoom left the window as first responder

Build `5a6149a`. Fourteen checks ran: Reduce Motion passed (canvas moving
before the toggle and still under it with the centre workers in attention;
renderer and companion followed; switch restored), DPR change passed,
display disconnect is MANUAL_EXTERNAL_REQUIRED.

`keyboard-navigation` failed with focus on the window for every key. A probe
on the same build located the cause: from a fresh launch plain Tab moves
through labelled controls without any click (Option-Tab does not move focus
in this web view), and after minimize/unminimize it still does, but after
fullscreen enter/exit or a zoom toggle the window itself becomes first
responder and Tab no longer reaches the office until a click. The desktop
shell now hands keyboard focus back to the web view (first responder only,
never raising the window) when the window resizes or becomes key; G16 walks
with plain Tab after those transitions.
