# Superseded: case D banner press found no banner (harness lock wait)

Stop took 859 ms; the press searched 20 s and found nothing. A manual repeat of raise -> Stop -> press found and pressed the banner and cold-started the companion, so the product path works. The difference was the runner taking the shared GUI lock only at the press, while another session's automation (a Python process holding /private/tmp/mac-gui-automation.lock) held it. Case D now holds the lock from the banner's submission through the press. Cited: ../../../c02-supervision/20261007T050045Z-prod/ (D).
