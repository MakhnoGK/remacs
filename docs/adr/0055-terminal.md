# 0055 — Terminal

Status: accepted, 2026-10-09 (design boards 41–47: TermFull, TermPanel, TermChips,
TermShared, TermCopy, TermShell, TermStates)

## Decision

- **A terminal plugin, `plugins/terminal`, on `alacritty_terminal`** (Apache-2.0): its
  emulator, grid, scrollback, selection, vi cursor and regex search; its `tty` for the
  pseudoterminal. Its event loop is not used: a thread of ours reads the terminal, so
  that what shells report and the emulator ignores is seen (below); another writes.
- **Shell agnostic**: `terminal.toml` names the shell and its arguments (`--login` by
  default); empty is `$SHELL`, else the login shell from `passwd`. A name is looked for on
  `PATH` and in `/opt/homebrew/bin`, `/usr/local/bin`, as an app started from the Finder
  has a short `PATH`. A shell that is not found says so in the terminal: ↵ opens
  `terminal.toml` at `shell`, `z` starts zsh once, `r` tries again.
- **Every terminal is a buffer**, `*term: <label>*`, read-only, drawn as a screen
  (`Registry::screens`, a `ScreenSource`: rows of cell runs, cursor, banner, notice,
  copy mode, search bar; the app lays the runs out at their columns). The label is a
  name given by `C-x r`, else the program's title (`user@host:` cut, directories
  shortened as fish's `prompt_pwd`), else the shell's directory. Chips list the
  terminals: in the panel's toolbar, or in the title bar over one in the window; running,
  exited (code), new output and bell show on them; `+N` lists those that do not fit.
- **Panel or window** (user's choice: both, default in config): `open-in = "panel" |
  "buffer"`. ⌘↩ moves the terminal between the two without restarting it; moved to the
  window, it closes the panel, so leaving it (`C-x b`) does not show it under the text
  as well. The panel shows one terminal and never the window's own.
- **One pane slot** (user's choice): panes are `PaneSource`s with a name; the shown one is
  the last selected that has something to show, else the first. A segmented switch in the
  toolbar picks it; a debugger stopping takes the slot. Which pane has the keys is the
  core's (`Editor::focus_pane`), so `C-x o` (`other-window`, files plugin) is shared; the
  debug pane's own toggle is gone.
- **Keys go to the shell**, encoded as xterm does (`Registry::key_sinks`, before any
  keymap), but for ⌘ chords and `editor-keys` (default `C-x`, `M-x`, `C-\``, `M-<f12>`,
  a prefix keeping its sequences). Keys (my choice, user delegated them), rebindable in
  `keys.toml`: `C-\`` and `M-<f12>` show, focus or hide; `s-t` new; `s-RET` panel ⇄
  window; `s-{`/`s-}` previous/next; while a terminal has the keys: `C-x k` close,
  `C-x r` rename, `C-x C-b` list, `C-x [` copy mode, `s-f` search, `s-c` copy the
  mouse's selection, `s-v` paste (bracketed when asked), `s-k` clear, `s-<up>`/`s-<down>`
  previous/next command.
- **Copy mode** reads the scrollback with Emacs keys on alacritty's vi cursor: `C-n`
  `C-p` `C-f` `C-b`, `M-f`/`M-b` (words end at punctuation, not at `/ . - _ ~`),
  `C-a`/`C-e`, `M-<`/`M->`, `C-v`/`M-v`, `C-SPC` mark, `M-w` copy and leave, `C-s`/`C-r`
  an incremental search (case-insensitive while lower case) counting all matches, `q`
  leaves. Its keys are taken in the key sink, like Emacs isearch, not keymaps.
- **Shell integration** without setup where the shell reports it: OSC 7 (directory) and
  OSC 133 (prompt, command start, exit code), as fish 4 prints. A prompt's cells are
  tagged with a hyperlink of our own (`remacs-prompt:<id>`), so its mark stays with its row
  through scrolling and rewrapping; marks are ✓ or ✗ with the code, a bar down the output,
  the code, duration and time on hover.
- **Ended shells stay** with their screen and a banner: ↵ restarts in the last
  directory, `q` closes; `close-on-exit` closes at once. **Closing kills** (user's
  choice: no restoring): SIGHUP to the shell's and the foreground process groups,
  SIGKILL after `kill-timeout`; remacs quitting hangs them up as the terminal closes.
- **Colors**: the theme's 16 ANSI colors, bright ones as on board 41; black behind text is
  the glass. OSC 52 copies reach the clipboard; programs asking for colors get the theme's.

## Later

- Shell integration for zsh and bash, links and the mouse for programs: done, ADR 0059.
- Dragging the panel's edge to resize it; renaming a chip in place.
- The panel under tables and outlines, not only under text.
