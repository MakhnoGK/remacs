# 0079 — One Emacs layout for the keys

Status: accepted, 2026-10-23 (issue #101)

remacs rethinks Emacs, so its keys are Emacs's; keys from IntelliJ and of remacs's own
had crept in beside them.

## Decision

- **Emacs's global keymap is the source**, and for ⌘ keys Emacs's macOS port
  (`ns-win.el`): ⌘← ⌘→ line start and end, ⌘L `goto-line`, ⌘K `kill-current-buffer`,
  ⌘U `revert-buffer`, ⌘J `exchange-point-and-mark`, ⌘F search, ⌘G ⌘⇧G the next and
  previous match in it. macOS habits Emacs leaves free stay (⌘D, ⌘⇧F, ⌘⇧P).
- **IntelliJ keeps only function keys**, its Windows and Linux ones where macOS's take
  Emacs keys: run ⇧F10, run at point ⌃⇧F10, choose a configuration ⌥⇧F10, debug at point
  ⌃⇧F9 (were ⌃R, ⌃⇧R, ⌥⇧R, ⌃⇧D). `C-r` is `search-buffer-backward` again and `C-M-r`
  its regexp twin; `raise-sexp` is M-x only, as in Emacs.
- **`C-c` and a letter is the user's**: no plugin binds one in any keymap. `tools` is
  M-x and the menu only; the debug pane's columns move under gud's `C-x C-a` (f, v, w).
- **One `next-error`** (`M-g n`, `M-g M-n`, ``C-x ` ``; `M-g p`, `M-g M-p`) goes to the
  source named last, like Emacs's `next-error-last-buffer`: a run or compile, or visiting
  its locations, names the console output (`Registry::next_error_sources`); going to the
  buffer's problems (`M-n`, `M-p`, the list) names them again, and they are where it
  goes otherwise. In a console the same keys always visit its own locations.
- **No clashes**: `Keymap::bind` panics when built-in keys are bound to two commands,
  or both to a command and as a prefix, in one keymap; a test checks `C-c` and a letter.
  `⌘↩` was both `open-line-below` and `terminal-maximize`; the terminal keeps it only
  while it has the keys.
- The terminal's `C-x r` (rename) shadowed the rectangle keys and is M-x only. Copy mode
  stays on `C-x [`, not vterm's `C-c C-t`: `C-c` goes to the shell, only `C-x` stays
  with remacs.
- Kept: IntelliJ's word steps (ADR 0072) are how words are found, not keys.

## Later

- The prefix argument (`C-u`, `M-<digit>`, `C--`), keyboard macros, `C-x C-b`,
  `C-x p`, `M-/`, and a test comparing the keymap with Emacs's.
- `TAB` indenting for the language.
