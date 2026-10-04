# 0021 — Git: conflicts, continue and abort

Status: accepted, 2026-10-04 (Git, step 7)

## Decision

- **What is in progress is read from Git's directory** with the status: `rebase-merge`
  or `rebase-apply` (step, total, onto, the branch, `REBASE_HEAD`, the next to-do line,
  `orig-head`) or `MERGE_HEAD`. Each unmerged file of at most 8 MB is read and cut at
  its markers (`conflict.rs`; diff3's base is understood).
- **The status shows it**: a banner across the top (Rebasing feature onto main, Step 2
  of 3, the commit applied, how many conflicts are left, Skip commit / Abort /
  Continue), facts Head · Onto · Applying (or Merging), an Unmerged section whose files
  say how many conflicts are left, a Resolved section for files that had conflicts in
  this step and are staged now, and Staged named Merged cleanly. The pill says
  `Rebase 2/3` and the conflicts left.
- **A file with conflicts, at point, is shown beside the pane** in place of the history:
  its lines around the open conflict, each conflict a card with both sides under their
  headers (upstream and your commit when rebasing, yours and theirs when merging), and
  lines already resolved here tinted and labelled how.
- **Keys over the status keys** while point is on such a file, like smerge: `o` and `t`
  keep a side, `b` both, `e` visits the file at the conflict, `n`/`p` go to the next or
  previous conflict, then the next unmerged file. A resolution is written through the
  buffer visiting the file if there is one (refused while it has changes), else to the
  file; once no markers are left the file is staged.
- **`C-c C-c`** goes on once nothing is unmerged: a merge commits; a rebase first opens
  the message of the commit applied, with the resolved files and the next step, then
  writes the message and runs `git rebase --continue`. **`C-c C-k`** aborts after asking,
  saying where the branch goes back to; `M-s` in the dialog or Skip commit skips.
- Core gains `Banner`, `ConflictView`, file notes in outline rows, `Tone::Success` and
  `FileState::Resolved`.
