use std::fs;
use std::path::PathBuf;

use remacs_core::{Buffer, Editor, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_text::Text;

use crate::{Failure, FormatPlugin, Request, deliver, edits, take_requests};

const UGLY: &str = "fn main(){\n    call(a,b);\n}\n";
const PRETTY: &str = "fn main() {\n    call(a, b);\n}\n";

fn editor() -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin, &FormatPlugin]);
    let path = PathBuf::from("/project/main.rs");
    editor.add_buffer(Buffer::new("main.rs", Text::from(UGLY), "Rust").with_file(path));
    editor
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn contents(editor: &Editor) -> String {
    let buffer = editor.window_buffer();
    buffer.contents(0..buffer.text().len())
}

#[test]
fn formats_the_buffer_keeping_point_on_its_character_in_one_undo_step() {
    let mut editor = editor();
    let b = UGLY.find('b').unwrap();
    editor.window_buffer_mut().set_point(b);
    press(&mut editor, "C-c C-f");
    let requests = take_requests(&mut editor, |_| true);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].range, None);
    assert_eq!(requests[0].mode, "Rust");
    deliver(&mut editor, requests[0].id, Ok(edits(UGLY, PRETTY)));
    assert_eq!(contents(&editor), PRETTY);
    assert_eq!(editor.window_buffer().point(), PRETTY.find('b').unwrap());
    press(&mut editor, "C-/");
    assert_eq!(contents(&editor), UGLY);
}

#[test]
fn the_region_is_formatted_when_active() {
    let mut editor = editor();
    let buffer = editor.window_buffer_mut();
    buffer.set_mark(4);
    buffer.activate_mark();
    buffer.set_point(9);
    press(&mut editor, "C-c C-f");
    let requests = take_requests(&mut editor, |_| true);
    assert_eq!(requests[0].range, Some(4..9));
}

#[test]
fn backends_take_only_what_they_accept() {
    let mut editor = editor();
    press(&mut editor, "C-c C-f");
    assert!(take_requests(&mut editor, |request| request.mode == "CSS").is_empty());
    assert_eq!(take_requests(&mut editor, |_| true).len(), 1);
}

#[test]
fn edits_for_a_changed_buffer_and_failures_are_not_applied() {
    let mut editor = editor();
    press(&mut editor, "C-c C-f");
    let request = take_requests(&mut editor, |_| true).remove(0);
    editor.window_buffer_mut().insert("// ");
    deliver(&mut editor, request.id, Ok(edits(UGLY, PRETTY)));
    assert_eq!(contents(&editor), format!("// {UGLY}"));
    assert_eq!(
        editor.echo().as_deref(),
        Some("Not formatted: the buffer changed meanwhile")
    );

    press(&mut editor, "C-c C-f");
    let request = take_requests(&mut editor, |_| true).remove(0);
    let failed = Failure::Failed("prettier: Unexpected token".into());
    deliver(&mut editor, request.id, Err(failed));
    assert_eq!(editor.echo().as_deref(), Some("prettier: Unexpected token"));
}

/// An editor visiting a file holding `UGLY`, formatting on save turned on.
fn formatting_on_save(name: &str) -> (Editor, PathBuf) {
    let path = std::env::temp_dir().join(format!("remacs-format-{}-{name}.rs", std::process::id()));
    fs::write(&path, UGLY).unwrap();
    let mut editor = Editor::new(&[&EditingPlugin, &FormatPlugin]);
    editor.visit_file(&path).unwrap();
    editor.window_buffer_mut().save().unwrap();
    editor.refresh();
    assert!(
        take_requests(&mut editor, |_| true).is_empty(),
        "off by default"
    );
    editor.run_command("format-on-save-mode");
    assert_eq!(editor.echo().as_deref(), Some("Formatting on save enabled"));
    (editor, path)
}

fn save(editor: &mut Editor) -> Request {
    editor.window_buffer_mut().save().unwrap();
    editor.refresh();
    let mut requests = take_requests(editor, |_| true);
    assert_eq!(requests.len(), 1);
    requests.remove(0)
}

#[test]
fn saved_files_are_formatted_and_saved_again() {
    let (mut editor, path) = formatting_on_save("again");
    let request = save(&mut editor);
    deliver(&mut editor, request.id, Ok(edits(UGLY, PRETTY)));
    assert_eq!(fs::read_to_string(&path).unwrap(), PRETTY);
    assert!(!editor.window_buffer().is_modified());
    editor.refresh();
    assert!(
        take_requests(&mut editor, |_| true).is_empty(),
        "its own save is not formatted again"
    );
    press(&mut editor, "C-/");
    editor.refresh();
    assert!(take_requests(&mut editor, |_| true).is_empty());
    fs::remove_file(path).unwrap();
}

#[test]
fn after_a_save_typing_and_files_nothing_formats_are_left_alone() {
    let (mut editor, path) = formatting_on_save("alone");
    let request = save(&mut editor);
    editor.window_buffer_mut().insert("// ");
    deliver(&mut editor, request.id, Ok(edits(UGLY, PRETTY)));
    assert_eq!(contents(&editor), format!("// {UGLY}"));
    let request = save(&mut editor);
    let unsupported = Failure::Unsupported("No formatter for this buffer".into());
    deliver(&mut editor, request.id, Err(unsupported));
    assert_eq!(editor.echo().as_deref(), Some("Formatting on save enabled"));
    assert_eq!(fs::read_to_string(&path).unwrap(), format!("// {UGLY}"));
    fs::remove_file(path).unwrap();
}
