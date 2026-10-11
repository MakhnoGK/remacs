use remacs_core::{Editor, Face, MessageKind, parse_sequence};
use remacs_editing::EditingPlugin;

use crate::{EchoPlugin, MESSAGES_BUFFER};

fn editor() -> Editor {
    Editor::new(&[&EditingPlugin, &EchoPlugin])
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn shown_text(editor: &Editor) -> String {
    let buffer = editor.window_buffer();
    buffer.contents(0..buffer.text().len())
}

#[test]
fn lists_every_message_with_its_command_and_follows_new_ones() {
    let mut editor = editor();
    editor.set_message("first");
    editor.set_error("went wrong\nsecond line");
    editor.set_message("first");
    editor.set_message("first");
    press(&mut editor, "C-h e");
    assert_eq!(editor.window_buffer().name(), MESSAGES_BUFFER);
    let lines: Vec<String> = shown_text(&editor)
        .lines()
        .map(|line| line[8..].to_owned())
        .collect();
    assert_eq!(
        lines,
        [
            "  —  first",
            "  —  went wrong",
            "     second line",
            "  —  first  ×2",
        ]
    );
    let last_start = shown_text(&editor).rfind("\n").map(|end| {
        let text = shown_text(&editor);
        text[..end].rfind('\n').map_or(0, |start| start + 1)
    });
    assert_eq!(Some(editor.window_buffer().point()), last_start);

    editor.set_done("later");
    editor.refresh();
    assert!(shown_text(&editor).ends_with("later\n"));
    let point = editor.window_buffer().point();
    assert!(shown_text(&editor)[point..].starts_with(&editor.message().unwrap().clock()));
}

#[test]
fn colors_errors_and_dims_the_columns() {
    let mut editor = editor();
    editor.set_error("broken");
    press(&mut editor, "C-h e");
    let text = shown_text(&editor);
    let faces = editor.decorations(0..text.len()).concat();
    let error = text.find("broken").unwrap();
    assert!(
        faces
            .iter()
            .any(|span| span.range == (error..error + 6) && matches!(span.face, Face::Console(_)))
    );
    assert!(
        faces
            .iter()
            .any(|span| span.range.start == 0 && span.face == Face::Comment)
    );
}

#[test]
fn keys_in_messages_clear_it_and_close_it() {
    let mut editor = editor();
    editor.set_message("one");
    press(&mut editor, "C-h e");
    assert_eq!(
        editor.text_controls().map(|controls| controls.hints.len()),
        Some(2)
    );
    press(&mut editor, "C-c C-k");
    editor.refresh();
    assert_eq!(shown_text(&editor), "");
    press(&mut editor, "q");
    assert_ne!(editor.window_buffer().name(), MESSAGES_BUFFER);
}

#[test]
fn messages_record_their_kind_and_the_command_running() {
    let mut editor = editor();
    press(&mut editor, "C-SPC");
    let message = editor.message_log().messages().back().unwrap();
    assert_eq!(message.text, "Mark set");
    assert_eq!(message.kind, MessageKind::Info);
    assert_eq!(message.command, Some("set-mark-command"));
}

#[test]
fn listing_the_keys_lasts_until_the_next_key() {
    let mut editor = editor();
    press(&mut editor, "C-h k");
    assert!(editor.key_hints_listed());
    press(&mut editor, "C-f");
    assert!(!editor.key_hints_listed());
}
