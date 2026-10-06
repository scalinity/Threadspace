# Failed attempt: hydration was awaited on the stalled subscription

Build `26ce728`; 13 of 14 cases passed. In
`retired-with-unconsumed-cached-frames` the product recovered as
specified: the stalled stream's subscription (48b24873-…, attached ms
1791286968110) never acknowledged its snapshot, the five-second hydration
deadline retired it, the shell removed and recreated the office view
(`OFFICE_VIEW_RECOVERED`, ms 1791286973635, `created` and `removed` true),
and the recreated view's subscription cbd55fa4-… hydrated at ms
1791286973779 with an equal projection.

The harness scored `newViewHydrated` false because it awaited hydration of
the first subscription attached after the trigger, which was the stalled
one and by design never hydrates. The check now awaits a `VIEW_HYDRATED`
at or after the recovery event and records the hydrated subscription.
