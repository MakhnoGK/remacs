# 0028 — Commenting lines

Status: accepted, 2026-10-04 (issue #1 editing experience, step 5)

## Decision

- **`M-;` and `s-/` run `comment-line`**, a toggle on point's line or on every line the
  region touches; a region ending at the start of a line leaves that line out.
- **Uncomment when every line is commented**, else comment them all, so a mixed selection
  becomes one commented block that the next press restores.
- **Aligned**: the comment start goes in at the smallest indentation of the lines, not at
  each line's own, so a commented block keeps its shape. Blank lines are left alone.
- **Syntax per mode** in core `Mode::comment` (`CommentSyntax { start, end }`, padding
  space included): `// ` for Rust, C, C++ and JS/TS; `/* … */` for CSS; `<!-- … -->` for
  HTML. Uncommenting removes one padding space beside each token if it is there.
  Fundamental mode has none and says so.
- **Markup embedded in code** has its own syntax, `Mode::markup_comment`: `{/* … */}` in
  JavaScript, JSX and TSX. It is used when the first commented line starts in markup
  (`SyntaxContext::Markup`, ADR 0027): a JSX element or its text, not an attribute or a
  `{…}` expression. Uncommenting recognises either syntax by the text.
- One undo step; point stays on its character; an active region stays active, so the
  toggle can be repeated.

## Open

- `M-;` is Emacs's `comment-dwim` (end-of-line comments) only by key; here it toggles lines.
