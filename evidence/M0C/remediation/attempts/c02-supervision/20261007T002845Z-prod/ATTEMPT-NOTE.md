# Superseded remediation run: c02-supervision/20261007T002845Z-prod

Build 8c82212 (pre-repair), `c02-supervision prod A,legacy`. Harness defect: the disposable Claude session was started between the notification's submission and the banner press, so the temporary banner had been dismissed and the press could not cold-start a companion. Case A was not evaluated. Fixed before the runner was committed (4c1d1c9): the press now follows the unregister at once, and the provider activity starts during the unsupervised instance's lifetime.
