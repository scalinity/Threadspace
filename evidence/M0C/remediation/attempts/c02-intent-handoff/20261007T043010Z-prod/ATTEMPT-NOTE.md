# Superseded: banner click started no companion (harness timing)

The cold start's banner press found no banner. Another session's GUI automation held the shared lock (/private/tmp/mac-gui-automation.lock) while the runner waited to press, and the banner expired. The runner now holds the lock from the banner's submission through the press, and falls back to a LaunchServices start (recorded as coldStartPath) when no banner is found. No scenario ran.
