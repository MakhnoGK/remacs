# 0020 — Git: rebase and merge

Status: accepted, 2026-10-04 (Git, step 6)

## Decision

- **One prompt for both**, opened by `r` (`git-rebase`), `m` (`git-merge`) or the Base
  fact's Rebase and Merge buttons, switched between them with `M-m`. It brings in the
  base first; `TAB` takes the next branch: the upstream, the other local branches, the
  remote ones. The field shows the branch, not typed, and says how many commits are new
  on it.
- **Rebasing lists the commits to replay**, oldest first: `p`, `s`, `f`, `d` pick,
  squash, fix up or drop the selected one, `M-↑`/`M-↓` move it. The Result line says
  how many commits will be on top of the branch. Edited, the steps are written as Git's
  to-do list (`GIT_SEQUENCE_EDITOR` copies them in, `GIT_EDITOR=true` keeps combined
  messages); untouched, a plain `git rebase` runs. Folding the first commit kept into
  nothing, or a rebase or merge with nothing to do, is refused before the prompt closes.
- **Merging lists the commits it brings in** and runs `git merge --no-edit`.
- Chips: Interactive (`M-i`, rebase), Autostash (`M-a`, on), Autosquash (`M-q`,
  rebase). A note says when changed files will be stashed and restored.
- The job runs on the monitor thread; a conflict stops it with Git's message, and the
  rebase or merge is left in progress for step 7.
- **Prompts gain**, in core: a keymap of their own over the others, a read-only input
  whose printable keys go to that keymap, a check of the answer for any prompt (moved
  from messages), step rows (`Candidate::action`), group counts, and completions that
  can change the label and subject and add a footnote and an outcome (`CommitChain`).
  `Tone::Special` colors folded commits.
- The status hints are the design's: Commit, Branch, Merge, Rebase, Stage, Unstage.
