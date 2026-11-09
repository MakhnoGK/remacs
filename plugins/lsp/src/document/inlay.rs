//! Inlay hints (ADR 0095): the types and parameter names servers show between the
//! characters of the lines in view, drawn dim and not part of the text.

use std::cell::Cell;
use std::ops::Range;

use lsp_types::{InlayHintKind, InlayHintLabel};
use remacs_core::{CommandResult, Editor, InlayHint};
use remacs_text::{Edit, Snapshot};
use serde::Deserialize;

use super::{Document, Documents};
use crate::client::Outgoing;
use crate::position::{Encoding, from_lsp};

pub(crate) const TOGGLE: &str = "lsp-toggle-inlay-hints";

/// A server's hint: before `offset` of the text it was found in, and its label with the
/// spaces the server pads it with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hint {
    pub offset: usize,
    pub kind: Option<InlayHintKind>,
    pub label: String,
}

/// `lsp.toml`'s `[inlay-hint-kinds]`: the kinds of hints shown, as servers tell them apart.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HintKinds {
    /// Of bindings, chained calls and closures' returns.
    types: bool,
    /// Parameter names at call sites.
    parameters: bool,
    /// The rest, like rust-analyzer's elided lifetimes and closing braces' names.
    other: bool,
}

impl Default for HintKinds {
    fn default() -> Self {
        Self { types: true, parameters: true, other: true }
    }
}

impl HintKinds {
    fn shows(&self, kind: Option<InlayHintKind>) -> bool {
        match kind {
            Some(InlayHintKind::TYPE) => self.types,
            Some(InlayHintKind::PARAMETER) => self.parameters,
            _ => self.other,
        }
    }
}

/// Whether hints show, as `lsp.toml` says unless toggled, and of which kinds; `changes`
/// counts how many times that changed, for every file to ask again.
#[derive(Clone, Copy, Default)]
pub(crate) struct Hinting {
    enabled: bool,
    toggled: bool,
    kinds: HintKinds,
    changes: u64,
}

impl Hinting {
    fn shown(&self) -> bool {
        self.enabled != self.toggled
    }
}

/// Takes `lsp.toml`'s `inlay-hints` and kinds.
pub(crate) fn configure(editor: &mut Editor, enabled: bool, kinds: HintKinds) {
    let hinting = editor.plugin_state::<Hinting>();
    if (hinting.enabled, hinting.kinds) != (enabled, kinds) {
        *hinting = Hinting { enabled, kinds, changes: hinting.changes + 1, ..*hinting };
    }
}

/// Hides the hints if they show, else shows them, until remacs quits.
pub(crate) fn toggle(editor: &mut Editor) -> CommandResult {
    let hinting = editor.plugin_state::<Hinting>();
    hinting.toggled = !hinting.toggled;
    hinting.changes += 1;
    let shown = hinting.shown();
    editor.set_message(if shown { "Inlay hints shown" } else { "Inlay hints hidden" });
    Ok(())
}

/// The menu item's title, by what the toggle would do.
pub(crate) fn toggle_title(editor: &Editor) -> &'static str {
    match editor.find_plugin_state::<Hinting>().is_some_and(Hinting::shown) {
        true => "Hide Inlay Hints",
        false => "Show Inlay Hints",
    }
}

pub(crate) fn hinting(editor: &mut Editor) -> Hinting {
    *editor.plugin_state::<Hinting>()
}

/// The hints `found` in `text`, as its bytes, in order; a label's parts joined.
pub(crate) fn decode(
    found: Vec<lsp_types::InlayHint>,
    text: &Snapshot,
    encoding: Encoding,
) -> Vec<Hint> {
    let mut hints: Vec<_> = found
        .into_iter()
        .map(|hint| {
            let label = match hint.label {
                InlayHintLabel::String(label) => label,
                InlayHintLabel::LabelParts(parts) => {
                    parts.into_iter().map(|part| part.value).collect()
                }
            };
            let pad = |padded: Option<bool>| if padded == Some(true) { " " } else { "" };
            // Drawn within one row.
            let label = label.replace(char::is_control, " ");
            Hint {
                offset: from_lsp(text, hint.position, encoding),
                kind: hint.kind,
                label: format!("{}{label}{}", pad(hint.padding_left), pad(hint.padding_right)),
            }
        })
        .collect();
    hints.sort_by_key(|hint| hint.offset);
    hints
}

/// A document's hints and what was asked for them.
#[derive(Default)]
pub(super) struct Hints {
    /// As of the buffer's text, of the kinds shown, in order.
    shown: Vec<Hint>,
    /// The version, the server's refreshes and the hintings when last asked, and the bytes
    /// of the lines asked.
    asked: Option<((i32, u64, u64), Range<usize>)>,
    /// Lines drawn outside those asked woke the editor, to ask for them.
    woken: Cell<bool>,
}

impl Hints {
    /// Keeps the hints on their text through `edit`: those at it are dropped until the
    /// server looks again, those after it move with it.
    pub(super) fn shift(&mut self, edit: Edit) {
        shift(&mut self.shown, edit);
    }
}

fn shift(hints: &mut Vec<Hint>, edit: Edit) {
    hints.retain_mut(|hint| match hint.offset {
        offset if offset < edit.start => true,
        offset if offset <= edit.old_end => false,
        offset => {
            hint.offset = edit.moved(offset);
            true
        }
    });
}

/// The bytes of `text` from the start of the first of `lines` to that of the line after.
fn line_bytes(text: &Snapshot, lines: Range<usize>) -> Range<usize> {
    let start = |line| text.line_range(line).map_or(text.len(), |range| range.start);
    start(lines.start)..start(lines.end)
}

impl Document {
    /// Asks for the hints of the `lines` in view, with as many more on each side, unless
    /// they were asked of this version, refresh and hinting; takes those found, of the
    /// kinds shown, moved onto the text since.
    pub(super) fn update_hints(&mut self, hinting: Hinting, lines: Option<Range<usize>>) {
        if !hinting.shown() {
            self.hints = Hints::default();
            self.client.take_inlay_hints(&self.path);
            return;
        }
        let text = self.sent_text();
        let asked = (self.version, self.client.inlay_hint_refreshes(), hinting.changes);
        if let Some(lines) = lines {
            let in_view = line_bytes(&text, lines.clone());
            let covered = self.hints.asked.as_ref().is_some_and(|(was, bytes)| {
                *was == asked && bytes.start <= in_view.start && in_view.end <= bytes.end
            });
            if !covered {
                let margin = lines.len();
                let range =
                    line_bytes(&text, lines.start.saturating_sub(margin)..lines.end + margin);
                let (uri, path) = (self.uri.clone(), self.path.clone());
                let text = text.clone();
                self.client.send(Outgoing::InlayHints { uri, path, text, range: range.clone() });
                self.hints.asked = Some((asked, range));
                self.hints.woken.set(false);
            }
        }
        if let Some((found_in, found)) = self.client.take_inlay_hints(&self.path) {
            let mut shown: Vec<_> =
                found.into_iter().filter(|hint| hinting.kinds.shows(hint.kind)).collect();
            if let Some(edit) = text.edit_from(&found_in) {
                shift(&mut shown, edit);
            }
            self.hints.shown = shown;
        }
    }
}

/// The lines of the window buffer in view, to ask hints for.
pub(super) fn lines_in_view(editor: &Editor) -> Range<usize> {
    let shown = editor.window_buffer().shown_lines();
    editor.viewport.first_line..editor.viewport.last_line(&shown) + 1
}

/// The hints the window buffer's servers found before offsets in `bytes`, its end too.
/// Lines drawn that were not asked for, as after scrolling with the mouse, which runs no
/// command, wake the editor once to ask for them.
pub(crate) fn inlay_hints(editor: &Editor, bytes: Range<usize>) -> Vec<InlayHint> {
    let Some(documents) = editor.window_buffer().local::<Documents>() else {
        return Vec::new();
    };
    let mut hints = Vec::new();
    for Hints { shown, asked, woken } in documents.list.iter().map(|document| &document.hints) {
        let unasked = asked
            .as_ref()
            .is_some_and(|(_, asked)| bytes.start < asked.start || bytes.end > asked.end);
        if unasked && !woken.replace(true) {
            editor.notifier()();
        }
        let first = shown.partition_point(|hint| hint.offset < bytes.start);
        let within = shown[first..].iter().take_while(|hint| hint.offset <= bytes.end);
        hints
            .extend(within.map(|hint| InlayHint { offset: hint.offset, text: hint.label.clone() }));
    }
    hints
}

#[cfg(test)]
mod tests {
    use remacs_text::Text;
    use serde_json::json;

    use super::*;

    fn hint(offset: usize, kind: Option<InlayHintKind>, label: &str) -> Hint {
        Hint { offset, kind, label: label.into() }
    }

    #[test]
    fn hints_are_read_in_order_padded_and_follow_edits() {
        // `é` is two bytes and one UTF-16 unit.
        let text = Text::from("let é = f(1);\n").snapshot();
        let found = json!([
            { "position": { "line": 0, "character": 10 }, "kind": 2,
              "label": [{ "value": "n" }, { "value": ":" }], "paddingRight": true },
            { "position": { "line": 0, "character": 5 }, "kind": 1, "label": ": i32" },
            { "position": { "line": 0, "character": 13 }, "label": "a\nb", "paddingLeft": true },
        ]);
        let mut hints = decode(serde_json::from_value(found).unwrap(), &text, Encoding::Utf16);
        let (types, parameters) = (Some(InlayHintKind::TYPE), Some(InlayHintKind::PARAMETER));
        assert_eq!(
            hints,
            [hint(6, types, ": i32"), hint(11, parameters, "n: "), hint(14, None, " a b")]
        );

        // Typing at the parameter's hint drops it; the hint after moves.
        shift(&mut hints, Edit { start: 11, old_end: 11, new_end: 12 });
        assert_eq!(hints, [hint(6, types, ": i32"), hint(15, None, " a b")]);
        // Deleting what ends at a hint drops it too.
        shift(&mut hints, Edit { start: 5, old_end: 6, new_end: 5 });
        assert_eq!(hints, [hint(14, None, " a b")]);
    }

    #[test]
    fn kinds_left_out_are_not_shown() {
        let kinds = HintKinds { parameters: false, ..HintKinds::default() };
        assert!(kinds.shows(Some(InlayHintKind::TYPE)) && kinds.shows(None));
        assert!(!kinds.shows(Some(InlayHintKind::PARAMETER)));
    }

    #[test]
    fn the_toggle_flips_what_lsp_toml_says_and_asks_again() {
        let mut editor = Editor::new(&[]);
        configure(&mut editor, true, HintKinds::default());
        let first = hinting(&mut editor);
        assert!(first.shown());
        toggle(&mut editor).unwrap();
        let toggled = hinting(&mut editor);
        assert!(!toggled.shown() && toggled.changes > first.changes);
        assert_eq!(toggle_title(&editor), "Show Inlay Hints");
        // Turned off in lsp.toml, they show while toggled.
        configure(&mut editor, false, HintKinds::default());
        assert!(hinting(&mut editor).shown());
        assert_eq!(toggle_title(&editor), "Hide Inlay Hints");
    }

    #[test]
    fn the_lines_in_view_are_asked_for_and_their_hints_shown() {
        use std::time::{Duration, Instant};

        use remacs_core::Buffer;

        let root = std::env::temp_dir().join(format!("remacs-inlay-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("input");
        let message =
            |json: &str| format!("printf 'Content-Length: {}\\r\\n\\r\\n%s' '{json}'", json.len());
        let initialized =
            r#"{"jsonrpc":"2.0","id":0,"result":{"capabilities":{"inlayHintProvider":true}}}"#;
        // The first request the server is sent, as it offers nothing else.
        let hints = r#"{"jsonrpc":"2.0","id":1,"result":[{"position":{"line":0,"character":5},"label":": i32","kind":1}]}"#;
        let script = format!(
            "{}; exec 3<&0; cat <&3 > {input} & \
             until grep -q textDocument/inlayHint {input} 2>/dev/null; do sleep 0.02; done; {}; wait",
            message(initialized),
            message(hints),
            input = input.display(),
        );
        let catalog = format!(
            "[[servers]]\nname = \"hinting\"\ncommand = \"sh\"\nargs = [\"-c\", {script:?}]\n\
             languages = {{ Fundamental = \"plaintext\" }}\n"
        );
        let plugin: &'static crate::LspPlugin =
            Box::leak(Box::new(crate::LspPlugin::new(catalog.leak())));
        let mut editor = Editor::new(&[plugin]);
        let buffer = Buffer::new("a.txt", Text::from("let x = 1;\n"), "Fundamental");
        editor.add_buffer(buffer.with_file(root.join("a.txt")));
        let started = Instant::now();
        let shown = loop {
            editor.refresh();
            let shown = editor.inlay_hints(0..10);
            if !shown.is_empty() || started.elapsed() > Duration::from_secs(5) {
                break shown;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(shown, [InlayHint { offset: 5, text: ": i32".into() }]);
        let input = std::fs::read_to_string(&input).unwrap();
        let asked = input.split("Content-Length").find(|message| message.contains("/inlayHint"));
        let asked: serde_json::Value = asked
            .and_then(|message| serde_json::from_str(&message[message.find('{')?..]).ok())
            .unwrap_or_else(|| panic!("{input}"));
        // The line in view, before the window's first layout, a line more, and the end.
        let range =
            json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 1, "character": 0 } });
        assert_eq!(asked["params"]["range"], range);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn lines_are_asked_for_by_their_bytes() {
        let text = Text::from("a\nbc\nd").snapshot();
        assert_eq!(line_bytes(&text, 1..2), 2..5);
        assert_eq!(line_bytes(&text, 0..9), 0..6);
    }
}
