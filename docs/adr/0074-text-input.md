# 0074 — Text input through macOS input methods

Status: accepted, 2026-10-21 (issue #92)

## Decision

- **Typed text comes from the input method.** `EditorView` is gpui's
  `EntityInputHandler`, registered for the whole view while it has focus. A key that
  would self-insert (`Editor::dispatch_key` returns `KeyOutcome::Typed`) is not inserted
  but left to propagate, so macOS's input context turns it into text: composed kana and
  hanzi, a dead key's accent on the next letter, or the character as it is.
  `Editor::type_text` inserts that text one self-insert per character, so pairs,
  indentation, undo amalgamation and multiple cursors behave as for keys; a newline,
  as dictation gives, runs what `RET` binds. Text from elsewhere, like the emoji
  picker, is typed the same way where typing is allowed.
- **Bound keys still win**: a key a keymap binds, a key sink takes or a prompt answers
  is handled first and goes no further. Every key the editor took or rejected stops
  there, so an unbound Meta key is not typed as Option's character; only a lone
  unbound Super key goes on to the menus.
- **Held keys repeat** through self-insert, as before, rather than opening macOS's
  accent menu.
- **The input method sees only the marked text**, not the buffer (`text_for_range` is
  none). The marked text is drawn underlined at the cursor that takes typed text: over
  the text after point in the editor, with the cursor after it; before the cursor in a
  typing prompt and in a table's edited field. The candidate window is placed at that
  cursor's last drawn bounds. `unmarkText` accepts the marked text, as AppKit
  documents. A click, a scroll or a menu command, which the input method does not see,
  first drops the marked text and has the input method discard it: committed, it would
  land where point moved to, or change a list the click picks from.
- **Which Option is Meta** is `ui.toml` `[keyboard] meta`: `"both"` (default),
  `"left"` or `"right"`. The side is read from the current `NSEvent`'s device-dependent
  flags, which gpui's `Modifiers` leave out. An Option key that is not Meta types what
  the layout gives it, through the input method.

## Later

- Reconversion and other input-method features that read the buffer around point.
- Repeats skip the input method, so a held Space in a kana input mode types a
  full-width space and then ASCII ones.
- Long marked text near the editor's right edge is clipped: it is drawn over the text
  and sideways scroll follows point, not the marked text's end.
- While text is composed, gpui gives every key to the input method first, so a Meta
  chord then (e.g. a dead key, then `M-f`) is typed as Option's character by layouts
  that have one; committing the marked text before an Option key needs a vendored gpui
  patch, as gpui does not know which Option is Meta.
- IME in terminals, which take keys through their key sink.
