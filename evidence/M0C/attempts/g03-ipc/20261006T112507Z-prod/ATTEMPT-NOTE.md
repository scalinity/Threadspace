# Failed attempt: the suite's cancellation case followed large replies

Build `26ce728`. The in-view suite ran its twelve concurrent FleetPage
queries (limit 200; replies above 8 KiB) and then retired a throwaway
subscription. The desktop shell cannot tell whether a reply of 8 KiB or
more was fetched from the framework's per-view cache, so a subscription
retired within 30 s of one recreates the office view (SPEC §18.5):
`OFFICE_VIEW_RECOVERED`, reason `UNCONSUMED_DATA_ON_RETIREMENT`, retired
incarnation 3018836b-…, desktop.log ms 1791285911736, 0.5 s after the suite
started (companion `INTENT_DELIVERED` ms 1791285910869). The view running
the suite was destroyed before it could report, and the harness timed out
after 900 s with an empty run directory.

The product behaved as specified. The probe ordering was wrong: the
cancellation case now runs before any large reply reaches the view
(`apps/desktop/src/qualification/commands.ts`, commit 9548043).
