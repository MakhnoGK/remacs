# 0059 — Terminals give programs the mouse, the keys they tell apart, links and hooks

Status: accepted, 2026-10-11 (issues #15, #16, #28, #38, #39; follows ADR 0055)

## Decision

- **The mouse goes to programs that ask for it** (DECSET 1000/1002/1003, encodings 1006
  SGR, 1005 UTF-8, normal): buttons, drags, moves and the wheel (64/65) at the pointer's
  cell. Without it the wheel sends ↑/↓ on the alternate screen and scrolls the scrollback
  on the main one. **⇧ keeps the mouse for remacs** whatever the program asked: ⇧+wheel
  scrolls the scrollback, ⇧+drag selects.
- **The kitty keyboard protocol** for programs that ask (`CSI > flags u`, answered to
  `CSI ? u`), so ⇧↵, ⌃↵, ⌃I, Esc… reach them as keys of their own: flags 1, 4, 8 and 16;
  event types (2) are not reported. Claude Code turns extended keys on outright only for
  terminals it knows by `TERM_PROGRAM` and otherwise probes, so answering is what makes
  ⇧↵ break the line. Without the protocol keys stay xterm's. `kitty-keyboard = false`
  turns it off; xterm's `modifyOtherKeys` is not supported.
- **Links**: ⌘ over `file:line:col`, a URL or a program's OSC 8 hyperlink underlines it
  and turns the pointer into a hand; ⌘-click or ↵ in copy mode follows it. A file's
  links show only if the file exists from the shell's directory (OSC 7). Finding and
  following links is shared with the run console (`remacs_files::links`).
- **Focus events** (DECSET 1004): `CSI I` / `CSI O` when the terminal gets or loses the
  keys.
- **zsh and bash report their commands** like fish: remacs injects hooks that print OSC
  133 A/C/D and OSC 7. zsh starts with `ZDOTDIR` at ours, whose `.zshenv` puts the user's
  back and reads it. bash starts with `--rcfile` ours, never as a login shell (a login
  shell reads no rcfile, and Apple's bash 3.2 ignores `ENV` with `--posix`); the script
  reads `/etc/profile` and the first of `~/.bash_profile`, `~/.bash_login`, `~/.profile`
  when a login shell was asked for, else `~/.bashrc`. The scripts are written to
  `$TMPDIR/remacs-shell-integration`. `shell-integration = false` leaves shells alone.
- **Padding** around the text, `padding-x` and `padding-y`; the grid's size is what is
  left, so programs see the right one.

## Consequences

- The core's screen API takes the mouse as `ScreenPointer`: cell, modifiers, and press,
  drag, release (with the button), move, leave or wheel; `ScreenSource::scroll` is gone.
  `ScreenSource::padding` gives a screen its room; `Screen::pointing` asks for the hand.
- A bash login shell's `shopt login_shell` is off, and `~/.bash_logout` is not read.
- The shell's prompt is marked from `precmd`/`PROMPT_COMMAND`, not by rewriting `PS1`,
  so the typed command belongs to the prompt's mark until it runs, as before.
