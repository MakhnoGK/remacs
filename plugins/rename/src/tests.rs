use std::fs;
use std::path::{Path, PathBuf};

use remacs_core::{Editor, FileChange, FileEdits, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_minibuffer::MinibufferPlugin;

use crate::{RenamePlugin, Request, deliver_changes, deliver_name, take_requests};

const MAIN: &str = "fn main() {\n    helper();\n}\n";
/// In `helper`.
const POINT: usize = 18;

/// `main.rs` calling `lib.rs`'s `helper`, point on the call.
fn project(name: &str) -> (Editor, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("remacs-rename-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let (main, lib) = (root.join("main.rs"), root.join("lib.rs"));
    fs::write(&main, MAIN).unwrap();
    fs::write(&lib, "pub fn helper() {}\n").unwrap();
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &RenamePlugin]);
    editor.visit_file(&main).unwrap();
    editor.window_buffer_mut().set_point(POINT);
    (editor, main, lib)
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn type_text(editor: &mut Editor, text: &str) {
    for character in text.chars() {
        press(editor, &character.to_string());
    }
}

fn input(editor: &Editor) -> Option<String> {
    editor.minibuffer().map(|minibuffer| minibuffer.input_text())
}

/// Asks for the name at point and is told it is `helper`.
fn ask_name(editor: &mut Editor, main: &Path) {
    press(editor, "C-c C-r");
    let requests = take_requests(editor);
    let [Request::Name { id, path, offset }] = requests.as_slice() else {
        panic!("{requests:?}");
    };
    assert_eq!((path.as_path(), *offset), (main, POINT));
    deliver_name(editor, *id, Ok("helper".into()));
}

/// Renames `helper` to `assist` and returns the rename request's id.
fn rename_to_assist(editor: &mut Editor, main: &Path) -> u64 {
    ask_name(editor, main);
    assert_eq!(input(editor).as_deref(), Some("helper"));
    press(editor, "M-DEL");
    type_text(editor, "assist");
    press(editor, "RET");
    let requests = take_requests(editor);
    let [Request::Rename { id, new_name, .. }] = requests.as_slice() else {
        panic!("{requests:?}");
    };
    assert_eq!(new_name, "assist");
    *id
}

#[test]
fn renames_everywhere_and_saves_every_file_changed() {
    let (mut editor, main, lib) = project("everywhere");
    let buffer = editor.window_buffer_mut();
    buffer.set_point(MAIN.len());
    buffer.insert("// unsaved\n");
    buffer.set_point(POINT);
    let id = rename_to_assist(&mut editor, &main);
    let changes = vec![
        FileChange::Edit(FileEdits { path: main.clone(), edits: vec![(16..22, "assist".into())] }),
        FileChange::Edit(FileEdits { path: lib.clone(), edits: vec![(7..13, "assist".into())] }),
    ];
    deliver_changes(&mut editor, id, Ok(changes));

    assert_eq!(editor.echo().as_deref(), Some("Renamed helper to assist: 2 places in 2 files"));
    assert_eq!(fs::read_to_string(&main).unwrap(), "fn main() {\n    assist();\n}\n// unsaved\n");
    assert_eq!(fs::read_to_string(&lib).unwrap(), "pub fn assist() {}\n");
    assert!(!editor.window_buffer().is_modified());
}

#[test]
fn a_backend_that_cannot_rename_says_why_once() {
    let (mut editor, main, _) = project("failure");
    let id = rename_to_assist(&mut editor, &main);
    deliver_changes(&mut editor, id, Err("No language server here".into()));
    let said = "Could not rename helper: No language server here";
    assert_eq!(editor.echo().as_deref(), Some(said));
    deliver_changes(&mut editor, id, Ok(Vec::new()));
    assert_eq!(editor.echo().as_deref(), Some(said));
}

#[test]
fn the_name_is_not_read_once_point_moved() {
    let (mut editor, _, _) = project("moved");
    press(&mut editor, "C-c C-r");
    let Some(Request::Name { id, .. }) = take_requests(&mut editor).pop() else {
        panic!("no name request");
    };
    press(&mut editor, "C-f");
    deliver_name(&mut editor, id, Ok("helper".into()));
    assert_eq!(input(&editor), None);
}

#[test]
fn the_new_name_must_differ() {
    let (mut editor, main, _) = project("same");
    ask_name(&mut editor, &main);
    press(&mut editor, "RET");
    assert_eq!(input(&editor).as_deref(), Some("helper"));
    assert!(take_requests(&mut editor).is_empty());
}

#[test]
fn what_cannot_be_renamed_is_said() {
    let (mut editor, _, _) = project("refused");
    press(&mut editor, "C-c C-r");
    let Some(Request::Name { id, .. }) = take_requests(&mut editor).pop() else {
        panic!("no name request");
    };
    deliver_name(&mut editor, id, Err("Nothing to rename here".into()));
    assert_eq!(input(&editor), None);
    assert_eq!(editor.echo().as_deref(), Some("Nothing to rename here"));
}
