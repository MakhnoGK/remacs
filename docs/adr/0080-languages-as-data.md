# 0080 — Languages as data, and a file's own word on its mode

Status: accepted, 2026-10-23 (issue #102, steps 1–2)

The major modes lived in Rust constants (`modes.rs`), so a script without an extension,
a header holding C++ or a file saying `-*- mode: sh -*-` opened in the wrong mode, and
adding a language meant code.

## Decision

- **`plugins/languages/languages.toml`** holds the built-in modes: extensions, file
  names, `#!` programs, the names Emacs and vim give them (`aliases`), how files of no
  known name begin (`first-lines`), pairs and comment syntax. It is read once and leaked
  into `'static` modes, as the rest of the editor keeps mode names as `&'static str`.
  Grammars and their queries stay in Rust until the queries are our own (step 3).
- **A mode is chosen as Emacs `set-auto-mode` does** (`Modes::name_for_file`): a
  `-*- mode: … -*-` line (the first, or the second after `#!`) or a vim modeline in the
  first or last five lines; else the program the `#!` line runs, through `env`, without
  its version digits; else the file name or extension; else its first line (`<?xml`,
  `<!DOCTYPE html`). Only the file's ends are read, each line cut at 1 KB.
- **`modes.toml`** adds file names (or `*` and an ending) and `#!` programs ahead of the
  built-in ones, like `auto-mode-alist` and `interpreter-mode-alist`; core reads it
  beside `keys.toml`.
- Names for files only, with no text (scratch, previews, blame), still go by name.

## Later

- Comment syntax for more languages, per-language indentation, and grammar sources and
  revisions in `languages.toml` once grammars can be downloaded (step 6).
- Mode chosen again when a file's `#!` or modeline changes, as Emacs's `normal-mode`.
