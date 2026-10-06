# Failed attempt: the discovery check waited for a pass with nothing to commit

Build `a6ef67a`; 3 of 6 cases passed. The three companion SIGKILL cases with
the UI closed passed (system relaunch, one writer, resumed capture, startup
discovery pass). The three UI-death cases (Cmd-Q, SIGTERM, SIGKILL) each
kept the same companion incarnation and durably captured an attention item
while the UI was absent, and failed only `discoveryPassAfterUiDeath`.

The companion's discovery loop runs every five seconds but logs
`DISCOVERY_PASS` only when a pass commits a change or force-retries a
surface join (`apps/agent-macos/core/src/discovery.rs`, the
`outcome.change.is_some() || force_surface` guard). With no Claude session
starting or ending on the machine, no pass committed anything within the
12-second wait, so no line appeared. Earlier development runs passed this
check only because unrelated sessions were changing at the time.

The check now gives the loop a real change to find: after the UI exits it
starts one disposable Claude session in a harness-owned Terminal window
under `/private/tmp/ts-m0c-g09-*`, requires a `DISCOVERY_PASS` with
`started >= 1` and the companion's binding of that session to the window's
TTY, requires that no client other than the harness's own QUALIFICATION
connections connected meanwhile (no UI, no hook), and closes the window
after proving ownership.
