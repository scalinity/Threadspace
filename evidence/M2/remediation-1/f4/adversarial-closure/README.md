# Independent calculator counterexample closure

Status: **CALCULATOR_COUNTEREXAMPLES_CLOSED_NATIVE_EVIDENCE_INCOMPLETE**.

`../run-adversarial-closure.py` executed the current actual calculator against
the corrected synthetic fixture and the previously demonstrated attribution and
population failures. `summary.json` records exact calculator/test-source hashes,
the execution command, unit-test output hash, and hashes of every retained raw
fixture/result pair.

The positive synthetic fixture includes valid synthetic census seals and clock
qualification. It passes the calculation predicates while retaining
`nativeExecution: false` and `verdict: INCOMPLETE_OR_FAIL`. These fixtures are
not native timestamps, observations, census records or acknowledgements.

Eight negative variants correctly return `normalPathPass: false` with explicit
errors: wrong store generation; wrong observer runtime epoch; missing clock rate
qualification; null DOM identities; missing observer tail seal; wrong tail
digest; a missing tail despite an independently retained final census; and an
unsealed/unqualified export shape.

All **21** calculator unit tests also passed in this independent execution. The
exact tested calculator SHA-256 is
`6e08fb40e0ceb003b8d39e187dc7edd527bf9fa6dff2f824a089d246d9ff1f03`.

This closes the calculator's previously demonstrated ability to accept those
incomplete inputs. It does **not** implement or qualify independent native hook
capture census production, final observer counters/digest acknowledgement, or a
platform clock-rate bound. The separate observer producer repair and its actual
portable controls are recorded in `../observer-census/`; they are not proved by
these calculator fixtures. The pre-correction observer collector tail-loss
witness remains in `../attempts/`. Native F4 evidence remains incomplete until
the source populations and clocks are established on a source-matched native run.
