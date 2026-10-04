# 0019 — Git: branches

Status: accepted, 2026-10-04 (Git, step 5)

## Decision

- **`b` (`git-branch`)** in the status buffer reads a branch name, like
  `magit-branch-or-checkout`. A new name is offered first as Create, from the base
  chosen; the local and remote branches matching the input follow, the current one
  tagged, local ones saying how far they are from their upstream.
- **Choosing**: a local branch is switched to; a remote one through the local branch of
  its name, made to track it if there is none. Typing a remote branch's name offers no
  Create, as switching makes that branch.
- **Creating** runs `git branch --no-track` (or `--track` with Track upstream, `M-t`)
  from the base, records it as `branch.<name>.remacs-base`, which the status's Base
  reads first, and switches to it unless Switch to new branch (`M-s`) is off. A
  detached `HEAD` as the base is not recorded.
- **Prompts gain choices**: `Prompt::choices` are chips with a label and a value that a
  key (here `TAB`) or a click moves to the next one; the candidates are listed again,
  so Create says the new base. The base starts at the current branch, then the
  status's base, then every branch.
- New icons: Remote (a globe) and Create (a plus).

## Later

Creating a branch at a commit of the history (`M-RET` on the board) comes with
selecting commits; deleting and renaming branches if needed.
