# 0022 — Git: push and pull

Status: accepted, 2026-10-04 (Git, step 8)

## Decision

- **Outlines gain a toolbar**: buttons at the right of the title bar, each with an icon,
  a label, a count badge and an action, off while they cannot act. The status has Pull
  (commits behind its upstream) and Push (commits ahead), off while a rebase or merge
  is in progress or off a branch.
- **Prompts can open from the toolbar**: under its right end, a title in place of the
  field (`Prompt::title`), chips and words under it (`Prompt::summary`) and choices drawn
  as segments (`PromptChoice::segmented`).
- **`P` (`git-push`)** offers the upstream (`u`), or a new one, `<first remote>/<branch>`,
  pushed with `--set-upstream`, and Elsewhere… (`e`), a remote branch read next. It
  lists the outgoing commits; chips Force with lease (`M-f`), Push tags (`M-t`), Dry
  run (`M-n`). It pushes `HEAD:refs/heads/<branch>` to the remote named.
- **`F` (`git-pull`)** shows how far the branch and its upstream are apart as of the last
  fetch, Rebase / Merge / Fast-forward only (`r`, `m`, `TAB`), what the branch will look
  like and the incoming commits; chips Autostash (`M-a`, on) and Remember for this
  branch (`M-b`, writes `branch.<name>.rebase`). The mode starts from
  `branch.<name>.rebase`, then `pull.rebase`; rebasing when neither is set.
  Fast-forward only is refused before the prompt closes while the branches diverge.
- **Jobs can say they are done**: a push or pull says what it is doing when it starts
  and what it did when it succeeds (`Monitor::run_saying`, `Update::Done`); conflicts
  from a pull land in step 7's view.
- `CommitChain` gains incoming dots, in the base's color, and a note under it.
