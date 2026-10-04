use remacs_core::{Buffer, Editor, Key, parse_sequence};
use remacs_text::Text;

use crate::EditingPlugin;

fn editor_with(text: &str) -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin]);
    editor.add_buffer(Buffer::new("test", Text::from(text), "Fundamental"));
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
    let buffer = editor.buffer();
    buffer.contents(0..buffer.text().len())
}

#[test]
fn typing_inserts_at_point() {
    let mut editor = editor_with("world");
    type_text(&mut editor, "hello ");
    assert_eq!(contents(&editor), "hello world");
    assert_eq!(editor.buffer().point(), 6);
}

#[test]
fn vertical_motion_keeps_goal_column() {
    let mut editor = editor_with("abcdef\nab\nabcdef");
    press(&mut editor, "C-e C-n");
    assert_eq!(editor.buffer().point(), 9);
    press(&mut editor, "C-n");
    assert_eq!(editor.buffer().point(), 16);
    press(&mut editor, "C-n");
    assert_eq!(editor.buffer().point(), 16);
    assert_eq!(editor.echo().as_deref(), Some("End of buffer"));
}

#[test]
fn motion_steps_over_multibyte_characters() {
    let mut editor = editor_with("жёлтый");
    press(&mut editor, "C-f C-f");
    assert_eq!(editor.buffer().point(), 4);
    press(&mut editor, "DEL");
    assert_eq!(contents(&editor), "жлтый");
}

#[test]
fn word_motion_skips_punctuation() {
    let mut editor = editor_with("fn  main() -> жук");
    press(&mut editor, "M-f M-f");
    assert_eq!(editor.buffer().point(), 8);
    press(&mut editor, "M-f");
    assert_eq!(editor.buffer().point(), "fn  main() -> жук".len());
    press(&mut editor, "M-b M-b");
    assert_eq!(editor.buffer().point(), 4);
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
fn paging_moves_point_into_view() {
    let lines: String = (0..100).map(|line| format!("{line}\n")).collect();
    let mut editor = editor_with(&lines);
    press(&mut editor, "C-v");
    assert_eq!(editor.viewport.first_line, 8);
    assert_eq!(editor.buffer().point_line(), 8);
    press(&mut editor, "M-> M-v");
    assert!(editor.viewport.contains(editor.buffer().point_line()));
}

#[test]
fn region_is_killed_and_yanked_back() {
    let mut editor = editor_with("hello brave world");
    press(&mut editor, "M-f C-SPC M-f");
    assert_eq!(editor.buffer().region(), Some(5..11));
    press(&mut editor, "C-w");
    assert_eq!(contents(&editor), "hello world");
    assert!(editor.buffer().region().is_none());
    press(&mut editor, "C-e C-y");
    assert_eq!(contents(&editor), "hello world brave");
    press(&mut editor, "C-x h M-w");
    assert_eq!(editor.kill_ring.newest(), Some("hello world brave"));
}

#[test]
fn consecutive_kills_yank_as_one() {
    let mut editor = editor_with("one\ntwo\nthree");
    press(&mut editor, "C-k C-k");
    assert_eq!(contents(&editor), "two\nthree");
    press(&mut editor, "C-y");
    assert_eq!(contents(&editor), "one\ntwo\nthree");
    press(&mut editor, "M-> M-DEL M-DEL");
    assert_eq!(editor.kill_ring.newest(), Some("two\nthree"));
}

#[test]
fn yank_pop_cycles_through_earlier_kills() {
    let mut editor = editor_with("alpha beta ");
    press(&mut editor, "M-d C-f M-d C-e C-y");
    assert_eq!(contents(&editor), "  beta");
    press(&mut editor, "M-y");
    assert_eq!(contents(&editor), "  alpha");
    press(&mut editor, "C-a M-y");
    assert!(editor.minibuffer().is_some());
    editor.exit_minibuffer(false).unwrap();
    assert_eq!(contents(&editor), "beta  alpha");
}

#[test]
fn delete_removes_an_active_region_without_killing_it() {
    let mut editor = editor_with("keep drop");
    press(&mut editor, "M-f C-SPC C-e DEL");
    assert_eq!(contents(&editor), "keep");
    assert!(editor.kill_ring.is_empty());
}
