use std::thread;
use std::time::Duration;

use remacs_core::{Buffer, Editor, Face};
use remacs_text::Text;

use super::*;

const GRAMMARS: &[Grammar] = &[Grammar {
    mode: "Rust",
    language: tree_sitter_rust::LANGUAGE,
    highlights: &[tree_sitter_rust::HIGHLIGHTS_QUERY],
}];
static SYNTAX: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS);

fn rust_editor(source: &str) -> Editor {
    let mut editor = Editor::new(&[&SYNTAX]);
    editor.add_buffer(Buffer::new("main.rs", Text::from(source), "Rust"));
    editor
}

fn faces(editor: &Editor) -> Vec<(String, Face)> {
    let buffer = editor.window_buffer();
    editor
        .decorations(0..buffer.text().len())
        .concat()
        .into_iter()
        .map(|span| (buffer.contents(span.range), span.face))
        .collect()
}

/// Waits for the parser thread, as the UI does when it is notified.
fn wait_for(editor: &mut Editor, face: (&str, Face)) -> Vec<(String, Face)> {
    for _ in 0..500 {
        editor.refresh();
        let faces = faces(editor);
        if faces
            .iter()
            .any(|(text, found)| (text.as_str(), *found) == face)
        {
            return faces;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("no {face:?} in {:?}", faces(editor));
}

#[test]
fn highlights_after_the_background_parse() {
    let mut editor = rust_editor("fn main() { let x = 1; }\n");
    let faces = wait_for(&mut editor, ("main", Face::Function));
    assert_eq!(
        faces,
        [
            ("fn".into(), Face::Keyword),
            ("main".into(), Face::Function),
            ("let".into(), Face::Keyword),
            ("1".into(), Face::Constant),
        ]
    );
}

#[test]
fn edits_shift_highlights_before_the_reparse() {
    let mut editor = rust_editor("fn main() {}\n");
    wait_for(&mut editor, ("main", Face::Function));
    editor.window_buffer_mut().insert("/// Entry.\n");
    editor.refresh();
    assert!(faces(&editor).contains(&("main".into(), Face::Function)));
    wait_for(&mut editor, ("/// Entry.\n", Face::Docstring));

    editor.window_buffer_mut().undo();
    editor.refresh();
    assert_eq!(faces(&editor)[1], ("main".into(), Face::Function));
}

#[test]
fn other_modes_are_not_parsed() {
    let mut editor = Editor::new(&[&SYNTAX]);
    editor.add_buffer(Buffer::new(
        "notes",
        Text::from("fn main() {}"),
        "Fundamental",
    ));
    editor.refresh();
    thread::sleep(Duration::from_millis(20));
    editor.refresh();
    assert!(faces(&editor).is_empty());
}
