# 0076 — Regular expressions in search and replace

Status: accepted, 2026-10-23 (issue #89)

## Decision

- **A switch in every search prompt**: `C-s`, `M-%`, `project-search` and
  `project-query-replace` show a "Regexp" chip that `M-r` turns on and off, the matches
  found again at once. `C-M-s` (`search-buffer-regexp`) and `C-M-%`
  (`query-replace-regexp`) open the buffer's prompts with it on, like Emacs
  `isearch-forward-regexp` and `query-replace-regexp`. ADRs 0040 and 0045 left regular
  expressions out; this adds them.
- **Rust `regex` syntax** (the `regex` crate, already used by `align-regexp`), not
  Emacs': `(…)` groups, `\b`, `\d`. `^` and `$` match at line ends. Smart case as in
  the literal searches: case is ignored unless the pattern has an uppercase letter that
  does not follow a backslash, so `\W` and `\S` leave it ignored.
- **Groups in replacements** as the crate writes them: `$1`, `${name}`, `$0` for the
  whole match, `$$` for a dollar. The replacement prompt's placeholder says so. Each
  match is expanded from a copy of it and one character on each side, all that `^`, `$`
  and `\b` see, so replacing everything on one long line of minified code costs as much
  as its matches, not as many copies of the line. A match that would go on past that
  copy, as one cut at the end of what was searched does, is taken from the copy cut
  where it ended; one that still cannot be found again is left as it is, never emptied.
- **A replacement naming a group the regexp lacks is refused**, its prompt kept open:
  the crate takes every letter, digit and `_` after `$` as the name, so `$1_new` means a
  group `1_new` and would replace with nothing; the message points to `${1}_new`.
- **CRLF line ends**: `^` and `$` see `\r\n` too, as project files are searched as they
  are on disk.
- **Long lines**: the buffer is searched a 64 KiB block at a time from one character
  before what is searched, each match looked for in the 4 KiB after its start; lighting
  the matches on screen reads only that much past the lines drawn, however long they are.
- **An invalid pattern finds nothing and says why** in the prompt's note, e.g.
  "Invalid regexp: unclosed group", rather than failing.
- **Empty matches** (`^`, `$`, `x*`): like the crate's and Emacs', none is found right
  where the last match ended, nor inside a character, so `y`, `n` and `!` replace the
  same matches and always move on.
- **Project search**: the whole input is one pattern, spaces included, rather than words
  in any order. A file is searched in one piece, as it is read or mapped already.

## Not covered

- In a buffer, a match longer than 4 KiB is found in pieces.
- Emacs replacement syntax (`\1`, `\&`) and case conversion (`\U`) are not read.
