# Threadspace

## Finishing a branch

When the work on a branch is complete and verified, close the branch out:

1. Commit all remaining work on the branch and push it to `origin`.
2. Merge the branch into `main` and push `main`, so the next piece of work starts from the finished state.
3. Delete the merged branch locally and on `origin`; once merged it is stale, and keeping only live branches makes it obvious what is in progress.

"Complete" means the branch met its own exit criteria (for a milestone, its acceptance in `docs/MILESTONES.md`).
