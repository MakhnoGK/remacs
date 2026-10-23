# 0077 — Describing keys and commands

Status: accepted, 2026-10-23 (issue #95)

## Decision

- **`C-h k` (`describe-key`) reads a key sequence and tells its command** in the echo
  area: `<keys> runs <command>` over the command's description, or `<keys> is undefined`.
  The core reads it (`Editor::read_key_sequence`, like Emacs `read-key-sequence`): keys
  go through the active keymaps until they run a command or none, prefixes shown as
  typed; a key that would type its character is `self-insert-command`; C-g cancels.
- **`C-h f` and `C-h x` (`describe-command`) read a command name** and tell
  `<command> is bound to <keys>` (every global binding, `keys.toml` included) over its
  description.
- **The echo area answers, not a `*Help*` buffer**: each command's description is one
  sentence, and a message over one line opens whole with its key (ADR 0058) and stays in
  `*Messages*`.
- **`list-view-keys` moves to `C-h b`**, Emacs `describe-bindings`' key, giving `C-h k`
  to `describe-key`.
- **Help → Settings Reference…** (`view-default-settings`) shows a section's file of
  the defaults, every setting explained (#84), read-only in `<file> (defaults)`;
  unlike `write-default-settings` it writes nothing.

## Later

- A `*Help*` buffer, should descriptions grow past a sentence.
- Minor-mode and prompt keys in `describe-command`.
