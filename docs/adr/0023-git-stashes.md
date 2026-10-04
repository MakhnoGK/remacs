# 0023 — Git: stashes

Status: accepted, 2026-10-04 (Git, step 9)

## Decision

- **`z` (`git-stash`)** reads a message, optional, and stashes with it (`git stash push`).
  Chips under the field: Keep staged changes (`M-k`, on, `--keep-index`), Include
  untracked (`M-u`), Only staged (`M-s`, `--staged`), which hides the other two: Git
  refuses `--staged` with untracked files, and keeping the index would stash nothing
  away. The field counts the files the stash takes; the chips' row says what changed.
- **Prompts can list entries of their own** (`Prompt::list_keymap`): `TAB`
  (`minibuffer-next-field`) moves the keys between the input and the list, the field
  or the selected entry lit. There, printable keys and `RET` go to that keymap; the
  answer is always the input. A click on an entry selects it and moves there.
- **Candidates can be entries** (`Candidate::expansion`): an id chip, what it is and
  when; selected, it opens to show its items and buttons (`EntryAction`) that run the
  commands its keys run, a destructive one apart.
- **Each stash** lists its files with line counts: tracked ones from one `git log -g
  --diff-merges=first-parent --raw --numstat` over `refs/stash`, untracked ones from
  its third parent. `a` applies, `p` pops, `b` reads a name for `git stash branch`, `k`
  drops after a dialog, `RET` shows it. Apply and pop use no `--index`.
- **Showing a stash** unfolds it in the status buffer: its files, each with its diff
  (`git stash show --patch --include-untracked`), read when unfolded and kept by the
  stash's commit id. `TAB` or `RET` on a stash folds or unfolds it; `a`, `A` and `k`
  apply, pop and drop the stash at point.
- Stash commands run without `GIT_LITERAL_PATHSPECS`: Git stashes untracked files with
  the pathspec `:/`, which it would take as a file name.
