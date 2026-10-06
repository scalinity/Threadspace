# Failed attempt: wrong presence check, and a notification flood

Two harness defects, neither a product defect:

1. **Presence check read a bounded view.** The runner raised 4,879 attention
   items as its acknowledged records and then looked for them in a companion
   snapshot. With that many open items the snapshot exceeds the 512 KiB bound
   and is, correctly, a bounded initial view that lists only a prefix
   (SPEC §18.4). The "lostAcknowledged: 3952" figure counts items outside that
   prefix, not lost records. Replaced by re-admission: every acknowledged UUID
   must answer ALREADY_COMMITTED at its original cursor.
2. **Every acknowledged record posted a real notification.** Raising
   attention submits a notification, so the companion submitted ~3,570
   notifications between 07:06:33 and 07:07:17 local time (the rest were held
   back by its bounded outbox, `NOTIFIER_BACKLOGGED`). The runner now uses a
   qualification-only durable admission that creates no attention and posts
   nothing. The 4,882 qualification attention items were resolved with the
   reason "qualification cleanup: M0C G10 live crash loop raised fixture
   attention" through the journaled owner command, and the delivered
   notifications were removed by the companion's own qualification-only clear.

The second-writer refusal (exit 75, incumbent unchanged), the idempotent
owner-command retry across a companion crash (COMMITTED then
ALREADY_COMMITTED at cursor 8700) and the engine checks in summary.json are
valid observations from this run.
