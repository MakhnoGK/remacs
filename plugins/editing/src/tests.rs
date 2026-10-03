use remacs_core::{Buffer, Editor, Key, parse_sequence};
use remacs_text::Text;

use crate::EditingPlugin;

fn editor_with(text: &str) -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin]);
    editor.buffer = Buffer::new("test", Text::from(text), "Fundamental");
    editor.viewport.set_height(10);
    editor
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn type_text(editor: &mut Editor, text: &str) {
    for character in text.chars() {
        editor.handle_key(Key::parse(&character.to_string()).unwrap());
    }
}

fn contents(editor: &Editor) -> String {
    let text = editor.buffer.text();
    String::from_utf8(text.bytes_in(0..text.len())).unwrap()
}

#[test]
fn typing_inserts_at_point() {
    let mut editor = editor_with("world");
    type_text(&mut editor, "hello ");
    assert_eq!(contents(&editor), "hello world");
    assert_eq!(editor.buffer.point(), 6);
}

#[test]
fn vertical_motion_keeps_goal_column() {
    let mut editor = editor_with("abcdef\nab\nabcdef");
    press(&mut editor, "C-e C-n");
    assert_eq!(editor.buffer.point(), 9);
    press(&mut editor, "C-n");
    assert_eq!(editor.buffer.point(), 16);
    press(&mut editor, "C-n");
    assert_eq!(editor.buffer.point(), 16);
    assert_eq!(editor.echo().as_deref(), Some("End of buffer"));
}

#[test]
fn motion_steps_over_multibyte_characters() {
    let mut editor = editor_with("жёлтый");
    press(&mut editor, "C-f C-f");
    assert_eq!(editor.buffer.point(), 4);
    press(&mut editor, "DEL");
    assert_eq!(contents(&editor), "жлтый");
}

#[test]
fn newline_keeps_indentation() {
    let mut editor = editor_with("    let x = 1;");
    press(&mut editor, "C-e RET");
    type_text(&mut editor, "y");
    assert_eq!(contents(&editor), "    let x = 1;\n    y");
}

#[test]
fn consecutive_typing_undoes_as_one_step() {
    let mut editor = editor_with("");
    type_text(&mut editor, "abc");
    press(&mut editor, "RET");
    type_text(&mut editor, "de");
    press(&mut editor, "C-/");
    assert_eq!(contents(&editor), "abc\n");
    press(&mut editor, "C-/ C-/");
    assert_eq!(contents(&editor), "");
    press(&mut editor, "C-?");
    assert_eq!(contents(&editor), "abc");
}

#[test]
fn prefix_keys_wait_and_abort() {
    let mut editor = editor_with("text");
    press(&mut editor, "C-x");
    assert_eq!(editor.echo().as_deref(), Some("C-x-"));
    press(&mut editor, "C-g");
    assert_eq!(editor.echo().as_deref(), Some("Quit"));
    press(&mut editor, "C-x C-q");
    assert_eq!(editor.echo().as_deref(), Some("C-x C-q is undefined"));
}

#[test]
fn visited_file_starts_unmodified() {
    let path = std::env::temp_dir().join(format!("remacs-visit-{}", std::process::id()));
    std::fs::write(&path, "fn main() {}\n").unwrap();
    let mut editor = Editor::new(&[&EditingPlugin]);
    editor.visit_file(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(!editor.buffer.is_modified());
}

#[test]
fn quitting_a_modified_buffer_needs_confirmation() {
    let mut editor = editor_with("");
    type_text(&mut editor, "x");
    press(&mut editor, "C-x C-c");
    assert!(!editor.quit_requested());
    press(&mut editor, "C-x C-c");
    assert!(editor.quit_requested());
}

#[test]
fn paging_moves_point_into_view() {
    let lines: String = (0..100).map(|line| format!("{line}\n")).collect();
    let mut editor = editor_with(&lines);
    press(&mut editor, "C-v");
    assert_eq!(editor.viewport.first_line, 8);
    assert_eq!(editor.buffer.point_line(), 8);
    press(&mut editor, "M-> M-v");
    assert!(editor.viewport.contains(editor.buffer.point_line()));
}
