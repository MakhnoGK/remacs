# 0004 — Minibuffer, buffers and the kill ring

Status: accepted, 2026-10-04 (M3)

## Decision

- **Buffers** live in one list, most recently selected first; the first is shown in the
  window. Names are unique (`name<2>`). Each buffer keeps its `window_start`.
- **Minibuffer** input is an ordinary `Buffer`: while a prompt is open, `Editor::buffer()`
  returns it, so every editing command works in the prompt. Prompts never nest.
- **Keymaps** are looked up as a stack, highest priority first: `minibuffer_keymap` while a
  prompt is open, then the global map. The first map that binds a sequence wins.
- **Completion**: a prompt gets a source `Fn(&Editor, &str) -> Completions`, recomputed when
  the input changes. Matching is orderless substrings with smart ASCII case; ranked
  prefix-first, then shorter. A `category` (`command`, `file`, `buffer`) lets commands act on
  the selected candidate (`C-x k` in the buffer prompt). Answers are kept as history per
  category.
- **Questions** (`y-or-n-p`) are prompts without a source; `y`/`n` answer them directly.
- **Kill ring** is global, at most 120 entries; consecutive kills append to the newest
  entry. It syncs with the macOS clipboard: kills are written out after each key, and text
  copied elsewhere is taken in when the window becomes active.
- **Region** follows transient mark mode: edits deactivate the mark, and `DEL` deletes an
  active region without saving it.

## Plugins

- `minibuffer`: `M-x`, RET / M-RET / TAB / C-n / C-p in prompts.
- `files`: find-file (project files from the Git root, `.gitignore` respected through the
  `ignore` crate; `/` or `~` browses the file system), switch-to-buffer, kill-buffer,
  save-buffer, save-some-buffers, save-buffers-kill-terminal.
- `editing`: adds mark, region, kill/yank/yank-pop and word motion.

## Open

- find-file walks the tree synchronously on each invocation (capped at 100 000 files);
  large trees need a background index kept fresh with FSEvents.
- No `write-file` yet: buffers without a file cannot be saved.
- The "Recent" group of the buffer switcher needs persisted recent files.
