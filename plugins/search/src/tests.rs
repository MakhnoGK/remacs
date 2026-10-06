use std::fs;
use std::ops::Range;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use remacs_core::{Buffer, Editor, FUNDAMENTAL_MODE, Face, Settings, parse_sequence};
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

fn contents(editor: &Editor) -> String {
    let buffer = editor.window_buffer();
    buffer.contents(0..buffer.text().len())
}

/// Opens `M-%` from point with `from` replaced by `to`, at the first match.
fn replacing(offset: usize, from: &str, to: &str) -> Editor {
    let mut editor = editor_at(offset);
    press(&mut editor, "M-%");
    type_text(&mut editor, from);
    press(&mut editor, "RET");
    type_text(&mut editor, to);
    press(&mut editor, "RET");
    editor
}

#[test]
fn replacing_asks_at_each_match_and_undoes_at_once() {
    let mut editor = editor_at(0);
    press(&mut editor, "M-%");
    type_text(&mut editor, "total");
    assert_eq!(editor.minibuffer().unwrap().note(), Some("3 matches"));
    assert_eq!(lit(&editor).len(), 3);
    press(&mut editor, "C-g");

    let mut editor = replacing(0, "total", "count");
    assert_eq!(editor.window_buffer().point(), TEXT.find("total").unwrap());
    assert_eq!(lit(&editor)[0], (TEXT.find("total").unwrap(), true));

    press(&mut editor, "y");
    let after = contents(&editor);
    assert!(after.contains("let count = sum();"));
    assert_eq!(editor.window_buffer().point(), after.find("Total").unwrap());

    press(&mut editor, "n y");
    assert!(editor.minibuffer().is_none());
    assert!(lit(&editor).is_empty());
    assert_eq!(editor.echo().as_deref(), Some("Replaced 2 occurrences"));
    assert!(contents(&editor).contains("Total + count"));

    press(&mut editor, "C-/");
    assert_eq!(contents(&editor), TEXT);
}

#[test]
fn replacing_all_quitting_and_nothing_to_replace() {
    let start = TEXT.find("fn sum").unwrap();
    let mut editor = replacing(start, "total", "x");
    press(&mut editor, "!");
    assert_eq!(editor.echo().as_deref(), Some("Replaced 2 occurrences"));
    assert!(contents(&editor).contains("x + x") && contents(&editor).contains("let total"));

    let mut editor = replacing(0, "sum", "add");
    press(&mut editor, "y C-g");
    assert!(editor.minibuffer().is_none());
    assert_eq!(editor.echo().as_deref(), Some("Replaced 1 occurrence"));
    assert!(contents(&editor).contains("fn sum()"));

    let editor = replacing(start, "main", "start");
    assert_eq!(editor.echo().as_deref(), Some("No “main” after point"));
    assert!(editor.minibuffer().is_none());
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

/// A Git work tree: two files with matches, an ignored one and a binary one.
fn project(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("remacs-search-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::create_dir_all(root.join("src/target")).unwrap();
    fs::write(root.join(".gitignore"), "target/\n").unwrap();
    fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    let file = open(\"a\");\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/io.rs"),
        "pub fn open_file() {}\nfn open() {}\n",
    )
    .unwrap();
    fs::write(root.join("src/target/out.rs"), "open file\n").unwrap();
    fs::write(root.join("data.bin"), b"open\0file\n").unwrap();
    root.canonicalize().unwrap()
}

/// Refreshes until the prompt lists something and says nothing more is coming.
fn wait_for_lines(editor: &mut Editor) -> Vec<(String, String, String)> {
    let started = Instant::now();
    loop {
        editor.refresh();
        let minibuffer = editor.minibuffer().unwrap();
        let candidates = minibuffer.candidates();
        if !candidates.is_empty() && minibuffer.note().is_none() {
            return candidates
                .iter()
                .map(|candidate| {
                    let group = candidate.group.as_deref().unwrap_or_default().to_owned();
                    (group, candidate.annotation.clone(), candidate.label.clone())
                })
                .collect();
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "no lines found"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_project_is_searched_for_lines_with_every_word_unsaved_buffers_as_they_are() {
    let root = project("words");
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &SearchPlugin]);
    editor.visit_file(&root.join("src/main.rs")).unwrap();
    let buffer = editor.window_buffer_mut();
    buffer.set_point(buffer.text().len());
    buffer.insert("// open the file\n");

    press(&mut editor, "M-s g");
    type_text(&mut editor, "file open");
    let lines = wait_for_lines(&mut editor);
    assert_eq!(
        lines,
        [
            (
                "src/main.rs".into(),
                "2".into(),
                "let file = open(\"a\");".into()
            ),
            ("src/main.rs".into(), "4".into(), "// open the file".into()),
            (
                "src/io.rs".into(),
                "1".into(),
                "pub fn open_file() {}".into()
            ),
        ]
    );

    press(&mut editor, "C-n C-n RET");
    let buffer = editor.window_buffer();
    assert_eq!(buffer.path(), Some(root.join("src/io.rs").as_path()));
    assert_eq!(buffer.point(), "pub fn ".len());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_search_stops_at_the_most_lines_the_settings_allow() {
    let root = project("most");
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &SearchPlugin]);
    let settings = [("search.toml", "project-max-matches = 2\n")]
        .map(|(name, text)| (PathBuf::from(name), text.to_owned()));
    editor.set_settings(Settings::from_files(settings));
    editor.visit_file(&root.join("src/io.rs")).unwrap();

    press(&mut editor, "M-s g");
    type_text(&mut editor, "open");
    let started = Instant::now();
    while editor.minibuffer().unwrap().note() != Some("First 2 lines") {
        assert!(started.elapsed() < Duration::from_secs(10), "never stopped");
        thread::sleep(Duration::from_millis(10));
        editor.refresh();
    }
    assert_eq!(editor.minibuffer().unwrap().candidates().len(), 2);
    fs::remove_dir_all(root).unwrap();
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
