use remacs_core::{Buffer, Editor, Key, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_text::Text;

use crate::MinibufferPlugin;

fn editor_with(text: &str) -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin]);
    editor.add_buffer(Buffer::new("test", Text::from(text), "Fundamental"));
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

fn selected(editor: &Editor) -> Option<String> {
    let candidate = editor.minibuffer()?.selected_candidate()?;
    Some(candidate.value.clone())
}

#[test]
fn m_x_runs_the_selected_command_and_remembers_it() {
    let mut editor = editor_with("one two");
    press(&mut editor, "M-x");
    type_text(&mut editor, "kill-w");
    assert_eq!(selected(&editor).as_deref(), Some("kill-word"));
    let candidate = editor.minibuffer().unwrap().selected_candidate().unwrap();
    assert_eq!(candidate.keys, ["M-d"]);
    assert_eq!(candidate.matches.first(), Some(&(0..6)));
    press(&mut editor, "RET");
    assert!(editor.minibuffer().is_none());
    assert_eq!(editor.buffer().contents(0..4), " two");
    assert_eq!(editor.last_command(), Some("kill-word"));

    press(&mut editor, "M-x");
    assert_eq!(selected(&editor).as_deref(), Some("kill-word"));
}

#[test]
fn minibuffer_input_is_edited_like_a_buffer() {
    let mut editor = editor_with("");
    press(&mut editor, "M-x");
    type_text(&mut editor, "undo-xx");
    assert_eq!(selected(&editor), None);
    press(&mut editor, "RET");
    assert_eq!(editor.echo().as_deref(), Some("No match"));
    press(&mut editor, "DEL DEL C-a C-d");
    assert_eq!(editor.minibuffer().unwrap().input_text(), "ndo-");
    press(&mut editor, "TAB");
    assert_eq!(editor.minibuffer().unwrap().input_text(), "undo-redo");
    press(&mut editor, "C-g");
    assert!(editor.minibuffer().is_none());
    assert_eq!(editor.echo().as_deref(), Some("Quit"));
}
