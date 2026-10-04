use std::fs;
use std::path::{Path, PathBuf};

use remacs_core::{Editor, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_minibuffer::MinibufferPlugin;

use crate::{Location, Request, XrefPlugin, deliver, take_requests};

/// `main.rs` calling `lib.rs`'s `helper`, each defined once in `lib.rs` and once in `mod.rs`.
fn project(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("remacs-xref-{}-{name}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let files = ["main.rs", "lib.rs", "mod.rs"].map(|file| root.join(file));
    fs::write(&files[0], "fn main() {\n    helper();\n}\n").unwrap();
    fs::write(&files[1], "// lib\npub fn helper() {}\n").unwrap();
    fs::write(&files[2], "pub fn helper() {}\n").unwrap();
    let [main, lib, module] = files;
    (main, lib, module)
}

fn editor_at(path: &Path, offset: usize) -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &XrefPlugin]);
    editor.visit_file(path).unwrap();
    editor.window_buffer_mut().set_point(offset);
    editor
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn place(editor: &Editor) -> (PathBuf, usize) {
    let buffer = editor.window_buffer();
    (buffer.path().unwrap().to_path_buf(), buffer.point())
}

#[test]
fn goes_to_the_definition_and_back() {
    let (main, lib, _) = project("single");
    let mut editor = editor_at(&main, 18);
    press(&mut editor, "M-.");
    assert_eq!(take_requests(&mut editor), [Request { id: 1, offset: 18 }]);
    let definition = Location {
        path: lib.clone(),
        offset: 14,
    };
    deliver(&mut editor, 1, vec![definition.clone(), definition]);
    assert_eq!(place(&editor), (lib, 14));
    press(&mut editor, "M-,");
    assert_eq!(place(&editor), (main, 18));
    press(&mut editor, "M-,");
    assert_eq!(editor.echo().as_deref(), Some("At start of xref history"));
}

#[test]
fn several_definitions_are_listed_to_choose_from() {
    let (main, lib, module) = project("several");
    let mut editor = editor_at(&main, 18);
    press(&mut editor, "M-.");
    let locations = vec![
        Location {
            path: lib,
            offset: 14,
        },
        Location {
            path: module.clone(),
            offset: 7,
        },
    ];
    deliver(&mut editor, 1, locations);
    let minibuffer = editor.minibuffer().unwrap();
    let rows: Vec<_> = minibuffer
        .candidates()
        .iter()
        .map(|candidate| (candidate.label.as_str(), candidate.annotation.as_str()))
        .collect();
    assert_eq!(
        rows,
        [("pub fn helper() {}", "2"), ("pub fn helper() {}", "1")]
    );
    press(&mut editor, "C-n RET");
    assert_eq!(place(&editor), (module, 7));
    press(&mut editor, "M-,");
    assert_eq!(place(&editor), (main, 18));
}

#[test]
fn answers_after_another_command_or_without_definitions() {
    let (main, lib, _) = project("late");
    let mut editor = editor_at(&main, 18);
    press(&mut editor, "M-. C-f");
    deliver(
        &mut editor,
        1,
        vec![Location {
            path: lib,
            offset: 0,
        }],
    );
    assert_eq!(place(&editor), (main.clone(), 19));
    press(&mut editor, "M-.");
    deliver(&mut editor, 2, Vec::new());
    assert_eq!(editor.echo().as_deref(), Some("No definitions found"));
    assert_eq!(place(&editor), (main, 19));
}
