use std::ops::Range;

use remacs_core::{Buffer, Editor, FUNDAMENTAL_MODE, Face, parse_sequence};
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

/// Spans lit by the search, with the chosen one's face told apart.
fn lit(editor: &Editor) -> Vec<(usize, bool)> {
    editor
        .decorations(0..TEXT.len())
        .into_iter()
        .flatten()
        .filter_map(|span| match span.face {
            Face::SearchMatch => Some((span.range.start, false)),
            Face::SearchCurrent => Some((span.range.start, true)),
            _ => None,
        })
        .collect()
}

#[test]
fn matches_are_lit_and_point_follows_the_selection_until_quitting() {
    let mut editor = editor_at(0);
    press(&mut editor, "C-s");
    type_text(&mut editor, "total");
    let [first, second, third] = [
        TEXT.find("total").unwrap(),
        TEXT.find("Total").unwrap(),
        TEXT.rfind("total").unwrap(),
    ];
    assert_eq!(editor.window_buffer().point(), first);
    assert_eq!(
        lit(&editor),
        [(first, true), (second, false), (third, false)]
    );

    press(&mut editor, "C-s");
    assert_eq!(editor.window_buffer().point(), second);
    assert_eq!(
        lit(&editor),
        [(first, false), (second, true), (third, false)]
    );

    press(&mut editor, "C-g");
    assert!(editor.minibuffer().is_none());
    assert_eq!(editor.window_buffer().point(), 0);
    assert!(lit(&editor).is_empty());

    press(&mut editor, "C-s");
    type_text(&mut editor, "total");
    press(&mut editor, "RET");
    assert_eq!(editor.window_buffer().point(), first);
    assert!(lit(&editor).is_empty());
}

fn found(query: &str, text: &Text, range: Range<usize>) -> Vec<usize> {
    let mut starts = Vec::new();
    Query::new(query).unwrap().find(text, range, |start| {
        starts.push(start);
        true
    });
    starts
}

#[test]
fn matches_across_chunks_are_found_once_each() {
    let mut text = Text::from("world aaa");
    text.insert(0, "hello ");
    text.insert(text.len(), "aa");
    assert_eq!(found("o w", &text, 0..text.len()), [4]);
    assert_eq!(found("aa", &text, 0..text.len()), [12, 14]);
    assert_eq!(found("aa", &text, 13..text.len()), [13, 15]);
    assert_eq!(found("aa", &text, 13..14), [] as [usize; 0]);
}
