use std::thread;
use std::time::Duration;

use remacs_core::{Buffer, Editor, Face, SyntaxContext};
use remacs_text::Text;

use super::*;

const GRAMMARS: &[Grammar] = &[
    Grammar {
        mode: "Rust",
        language: tree_sitter_rust::LANGUAGE,
        highlights: &[tree_sitter_rust::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: "JavaScript",
        language: tree_sitter_javascript::LANGUAGE,
        highlights: &[tree_sitter_javascript::HIGHLIGHT_QUERY],
    },
];
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

/// The context right after the first `|` in `marked`, once the tree is parsed.
fn context_after(mode: &'static str, marked: &str) -> SyntaxContext {
    let offset = marked.find('|').unwrap();
    let mut editor = Editor::new(&[&SYNTAX]);
    let text = marked.replace('|', "");
    editor.add_buffer(Buffer::new("x", Text::from(text.as_str()), mode));
    for _ in 0..500 {
        editor.refresh();
        if buffer_syntax::is_parsed(editor.window_buffer()) {
            return editor.syntax_at(offset);
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("never parsed");
}

#[test]
fn tells_strings_and_comments_from_code() {
    use SyntaxContext::{Code, Comment, String};
    for (marked, context) in [
        (r#"let s = "a|b";"#, String),
        (r#"let s = "|";"#, String),
        (r#"let s = "ab"|;"#, Code),
        (r#"let s = |"ab";"#, Code),
        ("let c = 'x|';", String),
        ("// note|\nfn f() {}", Comment),
        ("// note|", Comment),
        ("/* a| */ fn f() {}", Comment),
        ("/* a */| fn f() {}", Code),
        ("fn f() {}|", Code),
    ] {
        assert_eq!(context_after("Rust", marked), context, "{marked}");
    }
    for (marked, context) in [
        ("const s = `a|${b}`;", String),
        ("const s = `a${b|}`;", Code),
        ("const s = 'a|';", String),
    ] {
        assert_eq!(context_after("JavaScript", marked), context, "{marked}");
    }
}

#[test]
fn code_outside_buffers_is_highlighted_by_mode() {
    let editor = Editor::new(&[&SYNTAX]);
    let code = "pub fn len(&self) -> usize";
    let faces: Vec<_> = editor
        .highlight_code("Rust", code)
        .into_iter()
        .map(|span| (&code[span.range], span.face))
        .collect();
    assert!(faces.contains(&("fn", Face::Keyword)));
    assert!(faces.contains(&("usize", Face::Type)));
    assert_eq!(editor.highlight_code("Python", code), []);
}
