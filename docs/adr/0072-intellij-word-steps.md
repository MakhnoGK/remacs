# 0072 — Word steps and kills like IntelliJ's macOS keymap

Status: accepted, 2026-10-19

Issue #74: `M-f`, `M-b`, `M-d` and `M-DEL` follow IntelliJ's default macOS caret stops
(`EditorActionUtil`, `CaretStopOptions.DEFAULT`), replacing the #30 rule that a word owns
the whitespace to its left.

## Decision

- **A word is a run of identifier characters** (letters, digits, `_`, `$`): one
  `CharClass` for word steps, double clicks, iedit, transpose-words and avy.
- **Tokens split by character rules**: a word next to anything else, a quote, and a
  bracket, `,` or `;` next to a different character; other punctuation runs together
  (`->`, `!=`). Whitespace is never a stop. These stand in for the lexer tokens IntelliJ
  steps by; syntax-tree leaves can replace them if a language splits wrong.
- **Stops per command**, as IntelliJ's policies:
  - `forward-word`: the end of the next token, at most the next line's start;
  - `backward-word`: the start of the previous token, at most the previous line's end;
  - `kill-word`: the next token start or end, at most the line end, or from the line end
    the newline alone;
  - `backward-kill-word`: the previous token start, at most the line start, or from the
    line start the newline alone.
- **The kills take a quoted word with its quotes** (`|"test"` → `|`) when the opening
  quote is outside a string and the text after it inside one (`Editor::syntax_at`); a
  quote within a string, in a comment or in text no parser reads stays a stop of its own.
- `upcase-dwim` and the other case commands go to where `forward-word` goes.
- The `^`-marked strings of IntelliJ's `MoveToCaretStopTest.kt` are tests here, run from
  every offset.

## Later

- CamelHumps steps, selection-extending variants, configurable stop policies.
