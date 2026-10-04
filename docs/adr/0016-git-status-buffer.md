# 0016 — Git: the status buffer

Status: accepted, 2026-10-04 (Git, step 2)

## Decision

- **Core gains outlines**: an `Outline` (`Registry::outlines`) draws a buffer as a pane
  of facts above rows, one row per line: `Heading`s that fold, `File`s with their state
  and line counts, `Diff` lines and `Item`s. Point's line is the selected row; a click on
  a heading runs the outline's `toggle`, a double click on a row its `activate`. Facts
  carry `RefChip`s colored by `RefKind` (Head, Main, Branch, Remote, Tag), which the
  history and branch screens reuse. The UI fits as many rows as the window holds, rows
  being of different heights.
- **`C-x g` (`git-status`)** opens one status buffer per working tree, mode `Git`, like
  magit-status. Its lines stand for `Line`s (heading, file, hunk line, stash); the
  buffer is rewritten from each new snapshot, point staying on the same `Line`. Sections:
  Unmerged, Staged, Unstaged, Untracked, then Stashes, folded at first; empty ones are
  left out.
- **The monitor reads what views want**: `Wants` (details and the diffs of unfolded
  files) come with the start of a monitor and with `want`; then line counts are read per
  side (`--cached` and not), and each unfolded file's `git diff`. Writes are `Job`s the
  monitor runs between reads, so they never race them.
- **Keys**: `TAB` folds a section or a file's diff, `s`/`u` stage and unstage a section,
  file or hunk (a hunk is applied alone with `git apply --cached [--reverse]`), `k`
  discards unstaged changes or moves untracked files to the Trash after a dialog, `RET`
  visits the file at the diff line, `g` reads again, `q` shows the previous buffer.
- **Base, by best practice**: the branch named in `branch.<name>.remacs-base`, else
  GitHub CLI's `gh-merge-base`, else VS Code's `vscode-merge-base`; else the remote's
  default branch (`refs/remotes/<remote>/HEAD`, the local branch of that name when there
  is one); else `main` or `master`. Not shown on the base itself. "New commits" are those
  on the base that the branch lacks. Creating a branch (step 5) records its base.
- **The pill** of a status buffer shows the branch and `↑ahead ↓behind`, in the code
  font (`StatusSegment::code`).

## Later

History beside the pane (step 3); Rebase/Merge buttons and the other keys of the design
come with their steps.
