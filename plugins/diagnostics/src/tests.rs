use remacs_core::{Buffer, Editor, Face, Icon, StatusItem, parse_sequence};
use remacs_minibuffer::MinibufferPlugin;
use remacs_text::Text;

use crate::{Diagnostic, DiagnosticsPlugin, Severity, publish};

const SOURCE: &str = "fn main() {\n    point.advance(1);\n    let unused = 2;\n}\n";

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
    let mut editor = Editor::new(&[&MinibufferPlugin, &DiagnosticsPlugin]);
    editor.add_buffer(Buffer::new("main.rs", Text::from(SOURCE), "Rust"));
    let buffer = editor.window_buffer_mut();
    let text = buffer.text().snapshot();
    publish(
        buffer,
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
    assert_eq!(editor.echo().as_deref(), Some("problem with `advance`"));
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
    assert_eq!(editor.echo().as_deref(), Some("problem with `unused`"));
    press(&mut editor, "M-p");
    assert_eq!(point_text(&editor, 3), "let");

    let buffer = editor.window_buffer_mut();
    let text = buffer.text().snapshot();
    publish(
        buffer,
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
