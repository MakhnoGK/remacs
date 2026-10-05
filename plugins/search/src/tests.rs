use remacs_core::{Buffer, Editor, FUNDAMENTAL_MODE, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_minibuffer::MinibufferPlugin;
use remacs_text::Text;

use crate::SearchPlugin;
use crate::query::Query;

const TEXT: &str =
    "fn main() {\n    let total = sum();\n}\n\nfn sum() -> u32 {\n    Total + total\n}\n";

fn editor_at(offset: usize) -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &SearchPlugin]);
    editor.add_buffer(Buffer::new("main.rs", Text::from(TEXT), FUNDAMENTAL_MODE));
    editor.window_buffer_mut().set_point(offset);
    editor
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn type_text(editor: &mut Editor, text: &str) {
    for character in text.chars() {
        let key = if character == ' ' {
            "SPC".to_owned()
        } else {
            character.to_string()
        };
        press(editor, &key);
    }
}

fn listed(editor: &Editor) -> Vec<(String, String)> {
    editor
        .minibuffer()
        .unwrap()
        .candidates()
        .iter()
        .map(|candidate| (candidate.annotation.clone(), candidate.label.clone()))
        .collect()
}

#[test]
fn lines_that_match_are_listed_with_the_first_after_point_selected() {
    let mut editor = editor_at(TEXT.find("fn sum").unwrap());
    press(&mut editor, "C-s");
    type_text(&mut editor, "sum");
    assert_eq!(
        listed(&editor),
        [
            ("2".into(), "let total = sum();".into()),
            ("5".into(), "fn sum() -> u32 {".into()),
        ]
    );
    let minibuffer = editor.minibuffer().unwrap();
    assert_eq!(minibuffer.selected(), Some(1));
    assert_eq!(
        minibuffer.candidates()[0].matches,
        std::slice::from_ref(&(12..15))
    );

    press(&mut editor, "C-r RET");
    assert!(editor.minibuffer().is_none());
    assert_eq!(editor.window_buffer().point(), TEXT.find("sum").unwrap());
    assert_eq!(
        editor.window_buffer().mark(),
        Some(TEXT.find("fn sum").unwrap())
    );
}

#[test]
fn case_is_ignored_unless_the_input_has_an_uppercase_letter() {
    let mut editor = editor_at(0);
    press(&mut editor, "C-s");
    type_text(&mut editor, "total");
    let minibuffer = editor.minibuffer().unwrap();
    assert_eq!(listed(&editor).len(), 2);
    assert_eq!(minibuffer.candidates()[1].matches, [0..5, 8..13]);

    press(&mut editor, "C-a C-k");
    type_text(&mut editor, "Total");
    assert_eq!(listed(&editor), [("6".into(), "Total + total".into())]);

    press(&mut editor, "C-a C-k");
    type_text(&mut editor, "nothing");
    assert!(listed(&editor).is_empty());
    press(&mut editor, "RET");
    assert_eq!(editor.echo().as_deref(), Some("No match"));
    press(&mut editor, "C-g");
    assert_eq!(editor.window_buffer().point(), 0);
}

#[test]
fn matches_across_chunks_are_found_once_each() {
    let mut text = Text::from("world aaa");
    text.insert(0, "hello ");
    text.insert(text.len(), "aa");
    let query = Query::new("o w").unwrap();
    let mut starts = Vec::new();
    query.find(&text, |start| {
        starts.push(start);
        true
    });
    assert_eq!(starts, [4]);

    let query = Query::new("aa").unwrap();
    let mut starts = Vec::new();
    query.find(&text, |start| {
        starts.push(start);
        true
    });
    assert_eq!(starts, [12, 14]);
}
