# 0084 — Indentation from the syntax tree

Status: accepted, 2026-10-23 (issue #102, step 5a); extends ADR 0024

## Decision

- **Indent queries of our own** (clean room, as ADR 0081), `queries/<lang>/indents.scm`,
  for Rust, C, C++, JavaScript/JSX, TypeScript/TSX, Python, Go, Java, Ruby, Lua, Bash,
  YAML, JSON, HTML and CSS. Dialects join their base's first, as highlights do. Captures:
  - `@indent.begin` — the node's lines after its first are a level deeper than its first;
  - `@indent.align` — they line up with the node's start (a YAML mapping after `- `);
  - `@indent.end`, `@indent.branch` — a line starting with it (`}`, `end`, `else`, `case`
    in Go) stands at its begin node's level;
  - `@indent.open` — a blank line after a line ending with it (`{`, Python's `:`) is a
    level deeper.
- **The rule**: a line goes by the innermost begin or align node that starts above it. A
  blank line goes by an open token ending the line above, else a begin node around it,
  else the line above. A line inside a string or comment begun above keeps its blanks.
- **Core asks, syntax answers**: `Registry::indent_probes` give `Indentation { anchor
  line, deeper, align }`; the editing plugin turns that into blanks with the buffer's
  unit (ADR 0024). Lines of one command are indented in order, so an anchor reindented by
  it counts with its new blanks.
- **A fresh tree on the main thread**: right after an edit the parser thread has not
  caught up, so the probe parses the edited tree again incrementally, keeping the last
  result for the same text until the command ends, so no closed file's tree stays.
  Measured with `benches/indent.rs`: breaking a line and telling its indentation takes
  0.18 ms in a 9 000-line Rust file and 1.1 ms in a 90 000-line one; a text not parsed
  yet is parsed here only up to 1 MiB. Telling every line costs about 15 µs a line.
- **Python and YAML**: their indentation is their structure, so only blank lines are
  told, the one `RET` leaves among them; `TAB` on a code line adds blanks to the next
  tab stop, as before, and `indent-region` leaves their code lines.
- **Injected code** (scripts in HTML and Vue, fenced Markdown) is indented by its own
  language's query, from its tree edited to follow the text and parsed again in its
  ranges. Its top level lines up with its first code line, as its author put it: a
  page's scripts sit a level deeper than their tag, a Vue component's do not. The tag
  closing it, after its code, is the host's line.
- **The brackets stay the fallback**: where the tree around the line is broken (code
  being typed, an `ERROR` node), or for a mode without an indent query, `RET` indents
  as ADR 0024 says and `TAB` inserts to the next tab stop.
- **Keys**: `TAB` is `indent-for-tab-command` (the region's lines when it is active;
  point in the blanks goes to their end), `C-M-\` is `indent-region` (the region's lines
  only, as in Emacs, so a stray key does not reindent a large file for seconds), `M-i`
  is `tab-to-tab-stop`. `RET` indents the new line by the syntax, and first the line
  it leaves, as Emacs `electric-indent-mode` does: a `}`, `end` or `else` typed at the
  wrong level goes to its block's (code lines of Python and YAML stay where they are).
  Blank lines other than point's stay empty.

## Open

- An injected script's first line, with no code above it in its language, is left where
  it is written.
- Indent queries of one's own (ADR 0083) are not read yet.
