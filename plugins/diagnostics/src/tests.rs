use std::thread;
use std::time::Duration;

use remacs_core::{Buffer, Editor, Face, Icon, StatusItem, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_minibuffer::MinibufferPlugin;
use remacs_text::Text;

use crate::{Diagnostic, DiagnosticsPlugin, Severity, clear, publish};

const SOURCE: &str = "fn main() {\n    point.advance(1);\n    let unused = 2;\n}\n";
const CHECKER: &str = "rust-analyzer";

fn diagnostic(source: &str, text: &str, severity: Severity) -> Diagnostic {
    let start = source.find(text).unwrap();
    Diagnostic {
        range: start..start + text.len(),
        severity,
        message: format!("problem with `{text}`\nmore detail"),
        source: "rustc".into(),
    }
}

fn checked_editor() -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &DiagnosticsPlugin]);
    editor.add_buffer(Buffer::new("main.rs", Text::from(SOURCE), "Rust"));
    let buffer = editor.window_buffer_mut();
    let text = buffer.text().snapshot();
    publish(
        buffer,
        CHECKER,
        text,
        vec![
            diagnostic(SOURCE, "unused", Severity::Warning),
            diagnostic(SOURCE, "advance", Severity::Error),
        ],
    );
    editor
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn underlined(editor: &Editor) -> Vec<(String, Face)> {
    let buffer = editor.window_buffer();
    editor
        .decorations(0..buffer.text().len())
        .concat()
        .into_iter()
        .map(|span| (buffer.contents(span.range), span.face))
        .collect()
}

fn point_text(editor: &Editor, len: usize) -> String {
    let point = editor.window_buffer().point();
    editor.window_buffer().contents(point..point + len)
}

/// The messages in the bubble at point.
fn shown(editor: &Editor) -> Vec<String> {
    editor
        .popups()
        .into_iter()
        .flat_map(|popup| popup.notes)
        .map(|note| note.message)
        .collect()
}

fn rest(editor: &mut Editor) {
    thread::sleep(Duration::from_millis(600));
    editor.refresh();
}

#[test]
fn problems_show_once_point_rests_on_them_but_not_after_typing() {
    let mut editor = checked_editor();
    let advance = SOURCE.find("advance").unwrap();
    editor.window_buffer_mut().set_point(advance - 1);
    press(&mut editor, "C-f");
    assert!(shown(&editor).is_empty());
    assert!(editor.take_refresh_at().is_some());
    rest(&mut editor);
    assert_eq!(shown(&editor), ["problem with `advance`\nmore detail"]);
    assert_eq!(editor.popups()[0].anchor, advance);

    press(&mut editor, "C-f");
    assert_eq!(shown(&editor), [] as [String; 0]);
    press(&mut editor, "x");
    rest(&mut editor);
    assert!(shown(&editor).is_empty());

    press(&mut editor, "C-e");
    rest(&mut editor);
    assert!(shown(&editor).is_empty());
}

#[test]
fn problems_at_point_show_most_severe_first() {
    let mut editor = checked_editor();
    let buffer = editor.window_buffer_mut();
    let text = buffer.text().snapshot();
    let mut hint = diagnostic(SOURCE, "advance", Severity::Info);
    hint.message = "did you mean `forward`".into();
    publish(buffer, "oxlint", text, vec![hint]);
    press(&mut editor, "M-g n");
    assert_eq!(
        shown(&editor),
        [
            "problem with `advance`\nmore detail",
            "did you mean `forward`"
        ]
    );
}

#[test]
fn underlines_follow_edits_until_the_next_report() {
    let mut editor = checked_editor();
    assert_eq!(
        underlined(&editor),
        [
            ("advance".into(), Face::Error),
            ("unused".into(), Face::Warning)
        ]
    );
    let buffer = editor.window_buffer_mut();
    buffer.set_point(SOURCE.find("point").unwrap());
    buffer.insert("self.");
    editor.refresh();
    assert_eq!(underlined(&editor)[0], ("advance".into(), Face::Error));
}

#[test]
fn next_and_previous_problem_wrap_around() {
    let mut editor = checked_editor();
    press(&mut editor, "M-n");
    assert_eq!(point_text(&editor, 7), "advance");
    assert_eq!(shown(&editor), ["problem with `advance`\nmore detail"]);
    assert_eq!(editor.echo(), None);
    press(&mut editor, "M-n M-n");
    assert_eq!(point_text(&editor, 7), "advance");
    press(&mut editor, "M-p");
    assert_eq!(point_text(&editor, 6), "unused");
}

#[test]
fn next_and_previous_error_skip_infos() {
    let mut editor = checked_editor();
    let buffer = editor.window_buffer_mut();
    let text = buffer.text().snapshot();
    publish(
        buffer,
        CHECKER,
        text,
        vec![
            diagnostic(SOURCE, "main", Severity::Info),
            diagnostic(SOURCE, "advance", Severity::Error),
            diagnostic(SOURCE, "let", Severity::Info),
            diagnostic(SOURCE, "unused", Severity::Warning),
        ],
    );
    press(&mut editor, "M-g n");
    assert_eq!(point_text(&editor, 7), "advance");
    press(&mut editor, "M-g n");
    assert_eq!(point_text(&editor, 6), "unused");
    press(&mut editor, "M-g n");
    assert_eq!(point_text(&editor, 7), "advance");
    press(&mut editor, "M-g p");
    assert_eq!(point_text(&editor, 6), "unused");
    assert_eq!(shown(&editor), ["problem with `unused`\nmore detail"]);
    press(&mut editor, "M-p");
    assert_eq!(point_text(&editor, 3), "let");

    let buffer = editor.window_buffer_mut();
    let text = buffer.text().snapshot();
    publish(
        buffer,
        CHECKER,
        text,
        vec![diagnostic(SOURCE, "main", Severity::Info)],
    );
    press(&mut editor, "M-g n");
    assert_eq!(editor.echo().as_deref(), Some("No errors or warnings"));
}

#[test]
fn the_list_starts_at_the_next_problem_and_goes_to_the_chosen_one() {
    let mut editor = checked_editor();
    let after_advance = SOURCE.find("(1)").unwrap();
    editor.window_buffer_mut().set_point(after_advance);
    press(&mut editor, "C-c ! l");
    let minibuffer = editor.minibuffer().unwrap();
    let rows: Vec<_> = minibuffer
        .candidates()
        .iter()
        .map(|candidate| (candidate.icon, candidate.annotation.as_str()))
        .collect();
    assert_eq!(
        rows,
        [(Some(Icon::Error), "2:11"), (Some(Icon::Warning), "3:9")]
    );
    assert_eq!(minibuffer.selected(), Some(1));
    assert_eq!(minibuffer.candidates()[0].label, "problem with `advance`");

    press(&mut editor, "M-p RET");
    assert!(editor.minibuffer().is_none());
    assert_eq!(point_text(&editor, 7), "advance");
    assert_eq!(shown(&editor).len(), 1);

    press(&mut editor, "C-c ! l");
    assert!(editor.status_segments()[0].active);
    press(&mut editor, "C-c ! l");
    assert!(editor.minibuffer().is_none());
}

#[test]
fn the_status_pill_counts_by_severity() {
    let editor = checked_editor();
    let segment = &editor.status_segments()[0];
    assert_eq!(
        segment.items,
        [
            StatusItem::new(Some(Icon::Error), "1"),
            StatusItem::new(Some(Icon::Warning), "1"),
            StatusItem::new(Some(Icon::Info), "0"),
        ]
    );
    assert!(!segment.active);
}

#[test]
fn each_checker_replaces_and_clears_only_its_own_problems() {
    let mut editor = checked_editor();
    let buffer = editor.window_buffer_mut();
    let text = buffer.text().snapshot();
    publish(
        buffer,
        "oxlint",
        text.clone(),
        vec![diagnostic(SOURCE, "let", Severity::Warning)],
    );
    let ranges = |editor: &Editor| -> Vec<String> {
        underlined(editor)
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    };
    assert_eq!(ranges(&editor), ["advance", "let", "unused"]);

    let buffer = editor.window_buffer_mut();
    publish(buffer, CHECKER, text, Vec::new());
    assert_eq!(ranges(&editor), ["let"]);

    clear(editor.window_buffer_mut(), "oxlint");
    assert!(ranges(&editor).is_empty());
    assert_eq!(editor.status_segments().len(), 1);

    clear(editor.window_buffer_mut(), CHECKER);
    assert!(editor.status_segments().is_empty());
}
