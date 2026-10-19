# 0064 — A commit's changes, a file's log and untracked files' contents

Status: accepted, 2026-10-15 (issues #31, #33); the commit buffer redrawn 2026-10-20 (#73)

## Decision

- **A commit opens in a buffer of its own**, `Commit <id>`, mode `Git Commit`, like Emacs
  magit-revision, its diff against the first parent (`git show -M
  --diff-merges=first-parent`). Read once when opened, on a thread of its own, the buffer
  showing once read (#56); one buffer per commit. Diffs are kept up to `git.toml`
  `commit-max-bytes` in all; a file whose diff does not fit what is left says its size
  instead, so a vendored lockfile leaves the other files' diffs shown.
- **The buffer's text is the unified diff of all its files** (#73, layout of board 19),
  so every text motion, search, avy, mark and copy works on it: per file a header line
  (`M src/lib.rs  +3 −1`) on a faint band, then its hunks as git prints them, with their
  context (not the whole file: lines not in the buffer cannot be drawn). The gutter shows
  each line's old and new numbers (`LineExtra::number`), removed lines lie on a red band
  and added on a green one (`LineExtra::fill`, `ui.toml` `[diff] band-opacity`). Code is
  highlighted in each file's language by a decorator: the old side's lines and the new
  side's are each highlighted as code of their own, once per file as it is first drawn,
  with the highlighters taken from the editor when the commit opens (a decorator has only
  the buffer). One side of 2000 lines, the most a file shows, takes 10 ms
  (`cargo bench -p remacs-syntax --bench decorations -- diff_side`).
- **What a pair of lines changed is marked** (`Face::Added`/`Face::Removed`, a second
  decorator): in a run of as many added lines as removed, each removed line and the added
  line in its place, between the start and the end they share, like git's
  diff-highlight. Lines that share neither, or only indentation and punctuation, and runs
  of unequal length, are not marked.
- **Beside it, a column** (`TextControls::side`, `ui.toml` `[diff] side-width`): the facts
  (id and subject, author and date, each parent with an Open button), the message's body
  under a Message heading, and the files, the one at point selected and kept in sight; a
  click on a file puts its header at the top of the window. A window too narrow for both
  shows only the diff. Its rows are built when the commit opens and when TAB folds the
  message, shared with the app, which draws only those in sight and looks for the first
  no further back than rows fit: for a commit of 3000 files a frame's column went from
  742 µs and 9015 allocations to 0.35 µs and 13 (`cargo bench -p remacs-git --bench
  commit_view`), and finding its first row from 40–89 µs to 0.3–0.5 µs.
- **Keys in it**: `n`/`p` next/previous hunk, `N`/`P` or M-n/M-p next/previous file, TAB
  folds the message, RET visits the working-tree file at the diff line, `o` shows the file
  as the commit left it (as before it, for a file it deleted) in a read-only buffer
  `<name> @ <id>`, also read in the background, `q` goes back. Folding diffs is gone.
- **The history opens commits**: a click on one, or M-RET on the selected one. M-n / M-p
  select the next older / newer commit; the app scrolls the history to keep it in sight.
- **A file's log is the status history filtered to it** (user, 2026-10-15):
  `git-log-file` (C-x v l, Git ▸ File History) shows the status with only the commits
  that changed the visited file, `git log --follow`, unconnected like a search; a chip
  with the path clears it. A commit opened from it starts scrolled to that file.
- **Untracked files unfold into all added lines** (`git diff --no-index /dev/null`); an
  untracked directory into its files (`git ls-files --others --exclude-standard`), each
  unfolding the same way. Files over `git.toml` `untracked-max-bytes` are not read and
  say their size; binary ones say so; a directory lists `untracked-max-files` at most and
  counts the rest. Staging or discarding on any of their lines acts on the file.

## Consequences

- The history pane shows only in a window wide enough for it, so a file's log needs one
  too.
- A commit from before the file was renamed opens at its top, as the log knows the file
  by its current name only.
