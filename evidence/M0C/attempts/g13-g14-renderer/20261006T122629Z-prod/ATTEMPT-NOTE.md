# Failed attempt: the first accessibility read had no web content

Build `a6ef67a`, harness `8e5481c`. Both attestations were correct: the
packaged run attested `WEBGPU` with the WebGPU gate `PASS`; the forced run
attested `WEBGL2_COMPATIBILITY`, gate `FAIL`, `forcedCompatibility` true,
and `forced-webgl2.png` shows the badge "WEBGL2_COMPATIBILITY — diagnostic
backend; does not pass the WebGPU gate · gen 1". The negative run failed
only because its visible-badge check read no text.

WebKit builds a web view's accessibility tree in its web process on the
first request. A first `ax-tree` read of a running UI returned 115 nodes
(menus, the title-bar buttons and two empty groups); a second read two
seconds later returned 471, including the `AXWebArea`, 9 headings and 71
static texts. The harness now reads the tree through `Native::ax_tree`,
which asks again until the web area appears, and G16 warms the tree before
its keyboard focus walk.
