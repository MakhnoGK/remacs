use std::fs;
use std::path::{Path, PathBuf};

use remacs_core::{Editor, SymbolKind, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_minibuffer::MinibufferPlugin;

use crate::{
    Location, Request, Symbol, SymbolRequest, XrefPlugin, deliver, deliver_symbols, take_requests,
    take_symbol_requests,
};

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

fn symbol(
    name: &str,
    kind: SymbolKind,
    container: Option<&str>,
    path: &Path,
    offset: usize,
) -> Symbol {
    Symbol {
        name: name.into(),
        kind: Some(kind),
        container: container.map(Into::into),
        location: Location {
            path: path.to_path_buf(),
            offset,
        },
        line: usize::from(offset > 0),
    }
}

type Row = (String, String, Option<&'static str>, String);

fn symbol_rows(editor: &Editor) -> Vec<Row> {
    editor
        .minibuffer()
        .unwrap()
        .candidates()
        .iter()
        .map(|candidate| {
            let detail = candidate.detail.clone().unwrap_or_default().into_owned();
            (
                candidate.label.clone(),
                candidate.annotation.clone(),
                candidate.tag,
                detail,
            )
        })
        .collect()
}

#[test]
fn symbols_of_the_project_are_found_by_name_as_it_is_typed() {
    let (main, lib, module) = project("apropos");
    let mut editor = editor_at(&main, 18);
    press(&mut editor, "C-M-. h e l p");
    let requests = take_symbol_requests(&mut editor);
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests.last(),
        Some(&SymbolRequest {
            id: 4,
            query: "help".into(),
            from: main.clone(),
        })
    );
    assert_eq!(editor.minibuffer().unwrap().note(), Some("Searching…"));

    let symbols = vec![
        symbol("helper", SymbolKind::Function, None, &lib, 14),
        symbol("helper", SymbolKind::Function, Some("module"), &module, 7),
        symbol("health", SymbolKind::Variable, None, &lib, 0),
    ];
    deliver_symbols(&mut editor, 3, Ok(Vec::new()));
    assert_eq!(editor.minibuffer().unwrap().note(), Some("Searching…"));
    deliver_symbols(&mut editor, 4, Ok(symbols));
    let row = |name: &str, container: &str, kind, detail: &str| -> Row {
        (name.into(), container.into(), Some(kind), detail.into())
    };
    assert_eq!(
        symbol_rows(&editor),
        [
            row("helper", "", "function", "lib.rs:2"),
            row("helper", "module", "function", "mod.rs:2"),
            row("health", "", "variable", "lib.rs:1"),
        ]
    );
    assert_eq!(
        editor.minibuffer().unwrap().candidates()[0].matches,
        std::slice::from_ref(&(0..4))
    );

    // Until the backends answer again, what they found is narrowed to the new input.
    press(&mut editor, "e");
    assert_eq!(take_symbol_requests(&mut editor).len(), 1);
    assert_eq!(symbol_rows(&editor).len(), 2);

    press(&mut editor, "C-n RET");
    assert_eq!(place(&editor), (module, 7));
    press(&mut editor, "M-,");
    assert_eq!(place(&editor), (main, 18));
}

#[test]
fn without_a_backend_the_symbol_prompt_says_why_and_a_later_answer_replaces() {
    let (main, lib, _) = project("apropos-none");
    let mut editor = editor_at(&main, 0);
    press(&mut editor, "C-M-. x");
    let id = take_symbol_requests(&mut editor)[0].id;
    deliver_symbols(
        &mut editor,
        id,
        Err("No language server for this project".into()),
    );
    let minibuffer = editor.minibuffer().unwrap();
    assert!(minibuffer.candidates().is_empty());
    assert_eq!(
        minibuffer.note(),
        Some("No language server for this project")
    );

    // E.g. once a backend that answered while loading the project answers again.
    let found = vec![symbol("x", SymbolKind::Constant, None, &lib, 0)];
    deliver_symbols(&mut editor, id, Ok(found));
    assert_eq!(symbol_rows(&editor).len(), 1);
}

fn has_buffer(editor: &Editor, path: &Path) -> bool {
    editor
        .buffers()
        .iter()
        .any(|buffer| buffer.path() == Some(path))
}

#[test]
fn the_window_shows_the_symbol_selected_and_quitting_puts_it_back() {
    let (main, lib, module) = project("apropos-preview");
    let mut editor = editor_at(&main, 18);
    let found = || {
        vec![
            symbol("helper", SymbolKind::Function, None, &lib, 14),
            symbol("helper", SymbolKind::Function, None, &module, 7),
        ]
    };
    press(&mut editor, "C-M-. h");
    let id = take_symbol_requests(&mut editor)[0].id;
    deliver_symbols(&mut editor, id, Ok(found()));
    editor.refresh();
    assert_eq!(place(&editor), (lib.clone(), 14));
    press(&mut editor, "C-n");
    assert_eq!(place(&editor), (module.clone(), 7));
    press(&mut editor, "C-g");
    assert_eq!(place(&editor), (main.clone(), 18));
    assert!(!has_buffer(&editor, &lib) && !has_buffer(&editor, &module));

    press(&mut editor, "C-M-. h");
    let id = take_symbol_requests(&mut editor)[0].id;
    deliver_symbols(&mut editor, id, Ok(found()));
    press(&mut editor, "C-n RET");
    assert_eq!(place(&editor), (module, 7));
    assert!(!has_buffer(&editor, &lib));
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
