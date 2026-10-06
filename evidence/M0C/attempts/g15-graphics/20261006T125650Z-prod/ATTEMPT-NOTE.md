# Smoke attempt: the on-camera workers changed after the view re-subscribed

Build `a983837`, a three-minute smoke run. Ten of eleven lifecycle checks
passed; `domJournalAgreementPerMinute` cannot run in three minutes (the
sustained phase starts after the lifecycle phases), and
`pixelsChangingWhileLive` had 26 of 29 changed pairs.

The three unchanged pairs all followed the repeated hide/show phase. A
reproduction on the installed build showed the live generation rendering
about 60 frames a second and presenting them (a window resize re-laid the
canvas out), but with no attention markers on camera: the view had
re-subscribed during the toggles, the snapshot listed the two fixture
sessions first among 36, and the office lays workers out in view order
centred on the camera, so two idle workers were in the centre. The
renderer behaved correctly. G15 now re-checks the centre workers before
every live phase.
