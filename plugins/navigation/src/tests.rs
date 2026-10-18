use remacs_core::{Buffer, Editor, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_text::Text;

use crate::{NavigationPlugin, NavigationSettings};

fn editor_with(text: &str) -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin, &NavigationPlugin]);
    editor.add_buffer(Buffer::new("test", Text::from(text), "Fundamental"));
    editor.viewport.set_height(10);
    editor
}

fn lines(count: usize) -> String {
    (0..count).map(|line| format!("{line}\n")).collect()
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn place(editor: &Editor) -> (String, usize) {
    let buffer = editor.window_buffer();
    (buffer.name().to_owned(), buffer.point_line())
}

#[test]
fn back_and_forward_go_through_jumps_but_not_short_moves() {
    let mut editor = editor_with(&lines(100));
    press(&mut editor, "C-n C-n M->");
    editor.add_buffer(Buffer::new("other", Text::from("x"), "Fundamental"));
    press(&mut editor, "C-f");
    let mut step = |keys: &str| {
        press(&mut editor, keys);
        let (buffer, line) = place(&editor);
        format!("{buffer}:{line}")
    };
    assert_eq!(step("s-["), "test:100");
    assert_eq!(step("s-["), "test:2");
    assert_eq!(step("s-["), "test:2");
    assert_eq!(step("s-]"), "test:100");
    assert_eq!(step("s-]"), "other:0");
    assert_eq!(step("s-]"), "other:0");
}

#[test]
fn output_followed_and_mouse_scrolling_are_not_jumps() {
    let mut editor = editor_with(&lines(100));
    press(&mut editor, "C-f");
    // A console's point at its end follows what the program prints, between commands.
    let buffer = editor.window_buffer_mut();
    buffer.set_point(buffer.text().len());
    buffer.append_output(&lines(50));
    editor.refresh();
    editor.scroll(-60);
    press(&mut editor, "C-f s-[");
    assert_eq!(
        editor.echo().as_deref(),
        Some("At the start of the navigation history")
    );
}

#[test]
fn a_click_or_an_answer_in_another_buffer_is_a_jump_back_returns_from() {
    let mut editor = editor_with(&lines(100));
    press(&mut editor, "C-f");
    editor.set_point_at(60, 0);
    press(&mut editor, "s-[");
    assert_eq!(place(&editor), ("test".into(), 0));

    // As a definition asked for arrives in another file, outside a command.
    editor.add_buffer(Buffer::new("other", Text::from("x"), "Fundamental"));
    editor.refresh();
    press(&mut editor, "s-[");
    assert_eq!(place(&editor), ("test".into(), 0));
}

#[test]
fn back_returns_to_where_avy_jumped_from() {
    let mut editor = editor_with(&lines(30));
    editor.viewport.set_height(40);
    press(&mut editor, "M-g l");
    let label = editor
        .text_labels(0..40)
        .into_iter()
        .find(|label| label.offset == editor.buffer().text().line_start(25).unwrap())
        .unwrap()
        .text;
    for key in label.chars() {
        press(&mut editor, &key.to_string());
    }
    assert_eq!(editor.buffer().point_line(), 25);
    press(&mut editor, "s-[");
    assert_eq!(editor.buffer().point_line(), 0);
}

#[test]
fn another_command_puts_avy_labels_away() {
    let mut editor = editor_with("x x\n");
    press(&mut editor, "M-j x RET");
    assert_eq!(labels(&editor).len(), 2);
    editor.run_command("end-of-buffer");
    assert!(labels(&editor).is_empty());
    press(&mut editor, "a");
    assert_eq!(editor.buffer().contents(0..5), "x x\na");
}

#[test]
fn letters_that_lowercase_to_other_lengths_match_whole_characters() {
    let mut editor = editor_with("İẞ x\n");
    press(&mut editor, "M-j ß RET");
    assert_eq!(editor.echo().as_deref(), Some("No ß on screen"));
}

#[test]
fn last_changes_are_visited_one_per_line_latest_first() {
    let mut editor = editor_with(&lines(30));
    let type_at = |editor: &mut Editor, line: usize, text: &str| {
        let buffer = editor.buffer_mut();
        buffer.set_point(buffer.text().line_start(line).unwrap());
        // A command between, as moving there would be, so the typing undoes on its own.
        press(editor, "C-a");
        for character in text.chars() {
            press(editor, &character.to_string());
        }
    };
    type_at(&mut editor, 3, "a");
    type_at(&mut editor, 20, "bb");
    type_at(&mut editor, 10, "c");
    let line_after = |editor: &mut Editor, keys: &str| {
        press(editor, keys);
        editor.buffer().point_line()
    };
    assert_eq!(line_after(&mut editor, "M-<"), 0);
    assert_eq!(line_after(&mut editor, "C-x C-\\"), 10);
    assert_eq!(line_after(&mut editor, "C-x C-\\"), 20);
    assert_eq!(line_after(&mut editor, "C-x C-\\"), 3);
    assert_eq!(line_after(&mut editor, "C-x C-\\"), 3);
    assert_eq!(editor.echo().as_deref(), Some("No earlier change"));
    assert_eq!(line_after(&mut editor, "C-x C-|"), 20);
}

fn labels(editor: &Editor) -> Vec<(usize, String)> {
    editor
        .text_labels(0..100)
        .into_iter()
        .map(|label| (label.offset, label.text))
        .collect()
}

#[test]
fn typed_text_is_labelled_nearest_first_and_a_label_jumps_to_it() {
    let mut editor = editor_with("fox one\nfox two\nFox three\n");
    press(&mut editor, "C-n M-j f o RET");
    assert_eq!(
        labels(&editor),
        [(8, "a".into()), (0, "s".into()), (16, "d".into())]
    );
    press(&mut editor, "d");
    assert_eq!(editor.buffer().point(), 16);
    assert!(labels(&editor).is_empty());
    press(&mut editor, "C-u C-SPC");
    assert_eq!(editor.buffer().point(), 8);

    press(&mut editor, "M-j F RET");
    assert_eq!(editor.buffer().point(), 16, "capitals match exactly");
    press(&mut editor, "M-j q RET");
    assert_eq!(editor.echo().as_deref(), Some("No q on screen"));
    press(&mut editor, "M-j o RET C-g");
    assert!(labels(&editor).is_empty());
    assert_eq!(editor.echo().as_deref(), Some("Quit"));
}

#[test]
fn typing_labels_once_it_pauses() {
    let mut editor = editor_with("ab ab\n");
    editor.plugin_state::<NavigationSettings>().avy_timeout = 0;
    press(&mut editor, "M-j a b");
    assert!(labels(&editor).is_empty());
    editor.refresh();
    assert_eq!(labels(&editor), [(0, "a".into()), (3, "s".into())]);
}

#[test]
fn words_and_lines_are_labelled_at_their_starts() {
    let mut editor = editor_with("tab stop\ntop\n");
    press(&mut editor, "M-g w t");
    assert_eq!(labels(&editor), [(0, "a".into()), (9, "s".into())]);
    press(&mut editor, "s");
    assert_eq!(editor.buffer().point(), 9);
    press(&mut editor, "M-g l");
    assert_eq!(
        labels(&editor),
        [(9, "a".into()), (13, "s".into()), (0, "d".into())]
    );
}

#[test]
fn many_places_get_labels_of_two_keys() {
    let text = "x ".repeat(12);
    let mut editor = editor_with(&text);
    editor.plugin_state::<NavigationSettings>().avy_keys = "ab".into();
    press(&mut editor, "M-j x RET");
    let found = labels(&editor);
    assert_eq!(found.len(), 12);
    assert!(found.iter().all(|(_, label)| label.len() == 4));
    press(&mut editor, "a a");
    assert_eq!(labels(&editor).len(), 4);
    assert!(labels(&editor).iter().all(|(_, label)| label.len() == 2));
}

#[test]
fn paging_is_not_a_jump_and_a_new_jump_drops_the_way_forward() {
    let mut editor = editor_with(&lines(100));
    press(&mut editor, "C-v C-v C-v");
    press(&mut editor, "C-x C-SPC");
    assert_eq!(
        editor.echo().as_deref(),
        Some("At the start of the navigation history")
    );
    press(&mut editor, "M-> s-[ M-<");
    press(&mut editor, "s-]");
    assert_eq!(
        editor.echo().as_deref(),
        Some("At the end of the navigation history")
    );
}
