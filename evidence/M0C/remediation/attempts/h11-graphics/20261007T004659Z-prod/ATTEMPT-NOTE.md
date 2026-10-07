# Superseded remediation run: h11-graphics/20261007T004659Z-prod

Intermediate build a6da2f1 (development run). The pending-resource case passed. The pending-init case never held: the init gate overrode `requestAdapter` on the `navigator.gpu` instance captured at module load and the app's adapter request never reached it (an off-screen WKWebView probe of the pinned Three r186 did route through such an override). Fixed in c3f6de1 (gate on GPU.prototype, with a call counter).
