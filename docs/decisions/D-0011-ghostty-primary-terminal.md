# D-0011 — Ghostty as the primary terminal

**Status:** ACCEPTED design; integrated application qualification remains open.
**Scope:** SPEC §13, M2 primary workflow, M8 terminal boundaries, M14 acceptance.

## Decision

Ghostty is the required primary terminal for the personal local release.
Terminal.app remains supported. Automatic discovery and Return use Ghostty's
native scripting terminal ID and TTY; no shell wrapper, title matching, working
directory matching or manually paired surface is required.

A provider's independently established native Session and process incarnation
are joined through its kernel controlling device (`e_tdev`) to exactly one
native Ghostty terminal TTY whose character-device number (`st_rdev`) matches.
Persist the terminal's opaque native ID separately from its TTY and current
containing window/tab IDs. `GHOSTTY_SURFACE_ID` is a different namespace.
Ghostty's scripting PID is diagnostic: the observed build returns the
root-owned `login` wrapper for a terminal running a child command.

Every Return revalidates the provider, binding revision, process birth/image,
device, app generation and current native terminal ID. Native events target
the proven app process, because separate Ghostty installations can share the
same bundle identifier. The operation activates that process, focuses the
terminal, and reads back the front window, selected tab, focused terminal and
native frontmost application PID. Focus without app activation did not make
the candidate app frontmost in the native fixture. Activation alone never
establishes an exact surface.

Each Ghostty installation path is its own application. Discovery finds
Ghostty processes from kernel evidence alone, by executable path
`<bundle>.app/Contents/MacOS/ghostty` with bundle identifier
`com.mitchellh.ghostty`, wherever the bundle is installed. Apple events
addressed to a PID are delivered to the application registered at that PID's
bundle path, so a process is scripted only while it is the sole process
running its executable path. Two processes from one path cannot be told apart
by any event: both are refused with `GHOSTTY_AMBIGUOUS_PROCESSES` and neither
receives an event. Native readback alone cannot repair this, because a
misaddressed inventory would attribute another process's terminals.

A process's dictionary is known only for the executable image the kernel
actually maps (its mapped vnode, not the file now at its path). An
installation's dictionary is probed with the executable's file identity
unchanged across the probe and cached by that image. A bundle replaced in
place leaves an already-running process on its old image: its capability is
unknown (`GHOSTTY_RESTART_REQUIRED`) and it is never scripted.

Keep the existing two-second overall route deadline and fallback `NONE`.
Missing TTY support, ambiguous identity, denied permission, stale generation,
lost targets or incomplete readback leave Return unavailable with a specific
reason. Do not send terminal input or read terminal output to repair a join.

## Installed and candidate boundaries

The installed application is Ghostty **1.4.0, build 18457**, at
`/Applications/Ghostty.app`. `codesign --verify --deep --strict` passes and
`spctl` accepts it as a notarized Developer ID application from Mitchell
Hashimoto, team `24VZTF6M5V`. Its dictionary exposes readable `tty` (text) and
`pid` (integer) terminal properties, `split`, `new tab`, `input text`,
`send key` and `perform action`.

| Item | SHA-256 |
| --- | --- |
| 1.4.0 executable | `4b30ac9b049cdf9eb75b6d8c7764856c2b882de97c688dce8a1b80916b55271b` |
| 1.4.0 dictionary | `8695d2feff757e8aec077648fe8f3481b8b39c596c5279813627792f0860b85f` |

The earlier stable installation was **1.3.1, build 15212**, whose dictionary
(`0ac1bbd8e6fdc7e2ca36cee6b31bc95b2b9e6a4643560d6833c1e031aa022adf`) has no
TTY or PID and cannot supply the automatic join.

The selected candidate is the official signed and notarized prerelease
**0fe113a35, build 18413**, from Ghostty's `tip` release. It was downloaded as
GitHub release asset **676972445** and verified against the published digest.

| Item | SHA-256 |
| --- | --- |
| Downloaded DMG | `02130e25877263c8a30666295f64a18c151514b2cf6fefc08d6b7bd46aaea0a9` |
| Candidate executable | `7fce8aaff5986e28234d6af4d23907963ff2e46c3c6b251404df15cf595c9528` |
| Candidate dictionary | `8982503c57d2a04a820ac9be1e4199fada60e1c39ced340b053f8a883e29a69f` |

`codesign --verify --deep --strict` passed. `spctl --assess --type execute`
accepted the app as a notarized Developer ID application from Mitchell
Hashimoto, team `24VZTF6M5V`. The actual candidate dictionary contains readable
`tty` (`Gtty`, text) and `pid` (`Gpid`, integer) properties. These observations
qualify this candidate's inspected bytes, not future contents of the moving
`tip` download.

Existing Ghostty sessions stay intact during qualification and installation
preparation. An on-disk update cannot add capabilities to an already-running
old process. Any transition must retain the old app for rollback and identify
the running generation accurately; it must not quit unrelated live sessions.

A Ghostty process started before an in-place update keeps running its old
image. Such a process, and the sessions it hosts, stay without automatic
Return until that process is restarted; Threadspace never restarts it.

## Qualification evidence

The reusable runner is
[`ghostty_native.py`](../../tests/native/tools/ghostty_native.py), with fixed
PID-addressed fixture commands in
[`ghostty_fixture.js`](../../tests/native/tools/ghostty_fixture.js). It launches
a separate candidate process with configuration loading, state restoration and
automatic updates disabled. It acquires an initially empty app, creates only
its own surfaces, and checks native identities again before cleanup.

The fixture needs a candidate at its own bundle path. The capability run
below used a disk-image mount; a second process launched from an already
running installation's path is answered by that installation's process.
Session qualification therefore runs a disposable copy of the installed
bundle at a new path and proves the addressed process is the new empty one
before creating anything.

The successful capability run is retained privately at
`artifacts/ghostty-adaptation/native-capability-8/result.json`. Earlier failed
fixture attempts remain beside it and were cleaned up independently.

| Check | Result and scope |
| --- | --- |
| Signature and notarization | PASS for the candidate above. |
| Native hierarchy and TTY | PASS: two windows, three tabs, four distinct terminals; each TTY matched one owned fixture process's kernel controlling device. |
| Exact focus and readback | PASS: eight operations across all four panes, including split panes; app activation plus native focus; approximately 199–273 ms per fixture call. |
| Absent native ID | PASS: no focus was issued for the missing target. |
| Fixture cleanup | PASS: only acquired surfaces closed; candidate process exited. |
| Existing sessions | PASS: the separate stable process and its ten recorded terminal IDs were unchanged. |
| Threadspace discovery, persisted binding and Return | NOT_RUN in this capability fixture. |
| Packaged companion permission attribution | NOT_RUN in this capability fixture. |
| Provider Session verification | NOT_RUN: fixture commands were owned `sleep` processes, not provider Sessions. |

The primary Ghostty adapter still needs source-matched installed qualification,
the full M2 route/negative matrix and independent review. A capability fixture
does not accept M2 or the personal release.

## Sources

- [Official Ghostty scripting documentation](https://ghostty.org/docs/features/applescript)
- [Official prerelease installation and update behavior](https://ghostty.org/docs/install/pre)
- [TTY/PID scripting change](https://github.com/ghostty-org/ghostty/pull/11922)
- [Official tip release](https://github.com/ghostty-org/ghostty/releases/tag/tip)
