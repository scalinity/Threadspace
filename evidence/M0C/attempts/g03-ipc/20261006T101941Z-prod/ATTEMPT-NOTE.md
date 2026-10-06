# Failed attempt: in-view suite destroyed its own view

The suite's cancellation case connected a throwaway subscription and
disconnected it without acknowledging its snapshot. The snapshot frame was
larger than 8 KiB, so the pinned Channel delivered it through the per-view
fetch cache. On retirement the desktop shell therefore recreated the office
view (`OFFICE_VIEW_RECOVERED`, reason `UNCONSUMED_DATA_ON_RETIREMENT`,
retired incarnation d9975c24-…, desktop.log ms 1791281986282), as SPEC §18.5
requires, which destroyed the view running the suite before it could report.
The harness waited for the report and was stopped.

The product behaved as specified. The probe was wrong: a subscriber must
acknowledge what it applied before retiring. The case now acknowledges the
throwaway stream's last frame before disconnecting.
