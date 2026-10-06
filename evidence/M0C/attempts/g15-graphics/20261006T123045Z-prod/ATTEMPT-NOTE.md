# Failed attempt: a settled office has no visible idle motion

Build `a6ef67a`, harness `8e5481c`. Ten of eleven checks passed (minimize
disposal, return rebuild, 2D, hide during init, injected visible loss,
repeated loss, loss while hidden, repeated hide/show, reload with a
resource in flight, per-minute DOM/journal agreement). The run lasted
908 s.

`pixelsChangingWhileLive` failed: 8 of 124 live pairs changed. Cropping a
consecutive live pair (`frames/0047`, `0048`) to regions shows the scene
column identical (`changedFraction` 0) and only the inspector's DOM text
changing. The office's only idle motion is a worker's marker pulsing and
spinning while that worker needs attention (`packages/scene/src/controller.ts`:
the pulse uniform is 0 once acknowledged or idle); no worker on camera was
in the attention state, so the scene was correctly still. The whole-window
comparison counted DOM text as motion.

G15 now keeps the two workers nearest the camera centre in the attention
state with labelled qualification items (re-checked each minute, resolved
at the end) and compares only the canvas rectangle, located from the view's
own surface facts.
