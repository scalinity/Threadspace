# M0B commit map: pre-redaction → published SHAs

Before `m0b` was first pushed, its history was rewritten with `git filter-repo --replace-text`. The rewrite replaced the full session IDs of four unrelated Claude sessions with `unrelated-session-1…4` in every evidence file. Only files under `evidence/` changed: for every pair below, `git diff <old> <new> -- . ':(exclude)evidence'` is empty, and the final trees are byte-identical.

Evidence text, build logs and the installed binaries' `build-info.json` were written before the rewrite and cite the pre-redaction SHAs.

| Pre-redaction (cited in evidence) | Published | Commit |
| --- | --- | --- |
| `e048eef`, `7d86983`, `cfd3731`, `9e7cf29`, `a43fed0`, `46a2a04` | unchanged | implementation commits (no evidence files) |
| `35d7573` | `8c9a7d3601c589a6f1e9bcad7c515418fb5130f0` | Record M0B late-start discovery and hook ancestry evidence |
| `8980d34f3179d309b583d774d1bb27f09bdc9240` | `a732f8c9f6d90ddaaa11d857f47d26169d37d1dd` | Take Terminal's incarnation from the kernel, not LaunchServices |
| `fc6564199fcc8fb1c84b8895fa849fb567872c05` | `0f0d9f1779266e0ccc1e33025b0010378c55139d` | Require Terminal's frontmost-window readback for an exact route |
| `9dfd1763bc80dffc4d4e40498482c404af4257a9` | `1ed401b1161d87ddd4e591c1ec7540dd91e6f9f4` | Record M0B native qualification evidence and the GO candidate |

The installed candidate (`~/Applications/Threadspace.app`, companion sha256 `46aa6c86…`) was built from `fc65641`. Its application source is that of published commit `0f0d9f1`.
