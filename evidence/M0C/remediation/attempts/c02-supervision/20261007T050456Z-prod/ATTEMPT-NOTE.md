# Superseded: case E missed CORE_START lines (harness log parsing)

The product classified both probes UNKNOWN (diagnostics and OBSERVATION_STATE), but the runner missed their CORE_START lines: each previous probe, killed by the runner's SIGTERM between writing a log record and its newline, left the next probe's CORE_START on the same physical line, which the JSON-lines reader dropped. The reader now parses every JSON record on a line. The torn line is a preexisting companion-log defect (C-13 in the failure ledger). Cited: ../../../c02-supervision/20261007T050722Z-prod/ (E).
