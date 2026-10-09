# Recorded attempt: the recording itself blocked Terminal

This run started an uncut display recording (`screencapture -x -v -D1`) before opening its Claude window. From that point Terminal refused every scripting element query (`get id of every window`: -1728), and the runner waited in `wait_scriptable` before opening the window. After about two and a half minutes the operator stopped the recorder with SIGINT, at 00:06:02 local time. Terminal answered the next probe immediately. The run then completed all ten cycles, Mark handled and exit normally.

Corrections to `summary.json`:

- `recording.wallMs` (333,292) is the whole run. The movie covers only about 145 s, from the recorder's start to the operator's SIGINT, so it is not an uncut recording of the slice. It is kept privately (`m2-vertical-1791518617827.mov`, 129,822,203 bytes) and shows the refusal period, not the cycles.
- `terminalRefusals` reads 0 because, in this harness version, a wait inside `wait_scriptable` itself was not counted. The refusal lasted from the recorder's start until 00:06:02. Later runs count every wait.

Every other field stands. The cycles, Returns, Mark handled, latency and exit were all measured after the recorder stopped.
