# 0018 — Git: committing

Status: accepted, 2026-10-04 (Git, step 4)

## Decision

- **Prompts can write messages**: `Prompt::message` makes the input a subject with a
  description under it, a second buffer, so editing commands, undo and the kill ring
  work in both. The answer is the subject, a blank line and the description. While such
  a prompt is open `Registry::message_keymap` is active over the minibuffer's, like
  Emacs `git-commit-mode`: `RET` goes from the subject to the description and breaks
  its lines, `TAB` moves between them, `C-c C-c` answers, `M-p`/`M-n` bring back
  earlier messages. A `check` keeps the prompt open, the message kept, when it cannot be
  the answer yet.
- **`c` (`git-commit`)** in the status buffer opens it over the status, like
  `magit-commit`: the subject counted against 50 characters, the staged files with
  their states and line counts, the author and the branch, and chips for Sign-off
  (`M-s`), Amend last commit (`M-a`) and Skip hooks (`M-h`). `M-p` starts from the last
  commit's message, the last 20 being at hand.
- **The commit runs as a monitor job** (`git commit --file -`), so hooks never block
  the UI; their first line of errors is said. Amending with an empty message keeps the
  last one (`--no-edit`). Nothing staged, or an empty message when not amending, is
  refused before the prompt closes.
- List items may carry a file state and line counts, drawn like the status rows.

## Later

Committing everything unstaged too (`git commit -a`) and fixups come if needed.
