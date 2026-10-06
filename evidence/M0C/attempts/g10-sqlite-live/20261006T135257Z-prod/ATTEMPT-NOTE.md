# Failed attempt: the running companion was not the login item's

Build `5a6149a`. The second-writer case passed (a second instance found the
writer lock held, exited 75, incumbent unchanged). Round 1 killed the
companion and nothing relaunched it; the runner then failed with
`connect: Connection refused`.

The killed companion (PID 16719) was not the login item's. G06's last cases
stopped observation (unregistering the login item), and a notification
click then cold-started the agent app through LaunchServices
(`launchd`: `application.ai.scalinity.threadspace.agent.…`, "because launch
job demand"). G06's `enable` re-registered the login item, whose instance
(PID 16804) found the writer lock held, forwarded a pending notification
response and exited with status 0 (`forward.rs`, `EXIT_RUNNING`). launchd
relaunches a login item only after an unsuccessful exit, so it stayed down,
while the cold-started instance received `SetObservationEnabled` and became
the enabled writer. When G10 killed it, no companion remained.

This contradicts SPEC §18.9 (a successful exit only after observation stop
or at logout) and §19.5 (only the companion its login item started acquires
the writer lock and opens observation). Fixed in the companion and the
outer bootstrap; G06 now verifies the hand-over by native readback.
