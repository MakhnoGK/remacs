# 0064 — A commit's changes, a file's log and untracked files' contents

Status: accepted, 2026-10-15 (issues #31, #33)

## Decision

- **A commit opens in a buffer of its own**, `Commit <id>`, mode `Git Commit`, drawn as an
  outline like the status (user, 2026-10-15), like Emacs magit-revision: facts (the id and
  subject, the author and date, each parent with an Open button), the message's body under
  a Message heading, then its files, each unfolding into its diff against the first parent
  (`git show -M --diff-merges=first-parent`). Read once when opened; one buffer per commit.
- **Keys in it**: TAB folds, S-TAB all diffs, RET visits the working-tree file at the diff
  line, `o` shows the file as the commit left it (as before it, for a file it deleted) in a
  read-only buffer `<name> @ <id>`, `q` goes back.
- **The history opens commits**: a click on one, or M-RET on the selected one. M-n / M-p
  select the next older / newer commit; the app scrolls the history to keep it in sight.
- **A file's log is the status history filtered to it** (user, 2026-10-15):
  `git-log-file` (C-x v l, Git ▸ File History) shows the status with only the commits
  that changed the visited file, `git log --follow`, unconnected like a search; a chip
  with the path clears it. A commit opened from it starts with that file unfolded.
- **Untracked files unfold into all added lines** (`git diff --no-index /dev/null`); an
  untracked directory into its files (`git ls-files --others --exclude-standard`), each
  unfolding the same way. Files over `git.toml` `untracked-max-bytes` are not read and
  say their size; binary ones say so; a directory lists `untracked-max-files` at most and
  counts the rest. Staging or discarding on any of their lines acts on the file.

## Consequences

- The history pane shows only in a window wide enough for it, so a file's log needs one
  too.
- A commit from before the file was renamed opens with nothing unfolded, as the log
  knows the file by its current name only.
