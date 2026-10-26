# 0090 — Git: the status in tabs

Status: accepted, 2026-10-26 (issue #134)

The status had the changes as an accordion at the left and the history at the right
(ADR 0016, 0064). Now the right column shows one of three tabs, and the left follows it.

## Decision

- **Tabs atop the right column** (`Outline::tabs`): Changes, History (every branch) and
  Current branch, in place of the history's All branches / Current switch
  (`History::scopes` is gone). `1`, `2`, `3` show them, `l` goes round them; the Git menu
  has them under Status.
- **Changes**: the rows go at the right under the tabs, every file's diff shown, as the
  commit view shows a commit's (ADR 0064). At the left, under the facts, a side
  (`Outline::side`, a `TextSide`) lists the headings, files and stashes; point's file is
  selected there, a click goes to it. `n`/`p` go from hunk to hunk, `M-n`/`M-p` from
  file to file, `RET` and `o` visit, `TAB` still folds a section or a file, `s`/`u`/`k`
  act on what is at point. A conflicted file at point puts its conflicts in place of the
  rows (ADR 0021's view).
- **History**: the history at the right, the selected commit's files at the left as the
  buffer's lines, read with `git show --raw --numstat` in the background whenever the
  selection moves. `n`/`p` select a commit, `M-n`/`M-p` a file, `RET` and `o` open the
  commit at the file at point. `C-x v l` opens this tab.
- **Files are open by default**: a file Git lists shows its diff unless folded; an
  untracked directory and the files it lists only once unfolded. `S-TAB` folds or unfolds
  them all.
- **The Changes tab reads every diff at once**: one `git diff -M` for what is staged and
  one for what is not, kept up to `git.toml` `changes-max-bytes` each (a file past it
  shows its size), and each untracked file's as added lines. `read_patch` also splits at
  `diff --cc` and `* Unmerged path`, so conflicted files do not run into others.
- **The same keys in every tab** (user, 2026-10-26): `C-n`/`C-p` and the arrows move
  through the column at the right, a diff's lines or the commits; `n`/`p` by hunk or
  commit; `M-n`/`M-p` through the files at the left.
- **Narrow windows** keep the rows in the pane, the side and the history left out.
- **`N`/`P` are not the file keys** as in a commit: `P` is Push, so the status takes the
  commit view's other pair, `M-n`/`M-p`.

## Later

The Changes tab is drawn as rows, not as text: no syntax colors or line numbers in its
diffs yet.
