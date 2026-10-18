use std::fs;
use std::path::PathBuf;

use remacs_core::{Editor, FileEdits, parse_sequence};
use remacs_editing::EditingPlugin;

use crate::{Action, ActionsPlugin, Request, apply, deliver, failed, take_requests};

const MAIN: &str = "fn main() {\n    let x = 1;\n}\n";

/// `main.rs` visited with point on `x`, and an unvisited `lib.rs`.
fn editor(name: &str) -> (Editor, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("remacs-actions-{}-{name}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let (main, lib) = (root.join("main.rs"), root.join("lib.rs"));
    fs::write(&main, MAIN).unwrap();
    fs::write(&lib, "pub fn f() {}\n").unwrap();
    let mut editor = Editor::new(&[&EditingPlugin, &ActionsPlugin]);
    editor.visit_file(&main).unwrap();
    editor
        .window_buffer_mut()
        .set_point(MAIN.find('x').unwrap());
    (editor, main, lib)
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn action(title: &str, kind: &str, preferred: bool) -> Action {
    Action {
        title: title.into(),
        kind: kind.into(),
        preferred,
    }
}

fn rows(editor: &Editor) -> Vec<(String, Option<String>)> {
    editor
        .popups()
        .pop()
        .map(|popup| {
            popup
                .rows
                .into_iter()
                .map(|row| (row.label, row.detail.map(|detail| detail.into_owned())))
                .collect()
        })
        .unwrap_or_default()
}

fn contents(editor: &Editor) -> String {
    let buffer = editor.window_buffer();
    buffer.contents(0..buffer.text().len())
}

#[test]
fn the_chosen_action_edits_the_buffer_and_other_files() {
    let (mut editor, main, lib) = editor("apply");
    let x = MAIN.find('x').unwrap();
    press(&mut editor, "C-c C-a");
    assert_eq!(
        take_requests(&mut editor),
        [Request::List {
            id: 1,
            range: x..x,
            kind: None
        }]
    );
    deliver(
        &mut editor,
        1,
        vec![
            action("Add type", "rewrite", false),
            action("Rename to `_x`", "quick fix", true),
        ],
    );
    assert_eq!(
        rows(&editor),
        [
            ("Rename to `_x`".into(), Some("quick fix".into())),
            ("Add type".into(), Some("rewrite".into())),
        ]
    );
    press(&mut editor, "C-n RET");
    assert!(editor.popups().is_empty());
    assert_eq!(
        take_requests(&mut editor),
        [Request::Apply { id: 1, index: 0 }]
    );
    let type_end = x + 1;
    let changes = vec![
        FileEdits {
            path: main.clone(),
            edits: vec![
                (type_end..type_end, ": i32".into()),
                (x..x + 1, "y".into()),
                (0..0, "use lib::f;\n".into()),
                (0..0, "\n".into()),
            ],
        },
        FileEdits {
            path: lib.clone(),
            edits: vec![(0..0, "// used\n".into())],
        },
    ];
    apply(&mut editor, 1, changes).unwrap();
    assert_eq!(
        contents(&editor),
        "use lib::f;\n\nfn main() {\n    let y: i32 = 1;\n}\n"
    );
    let point = editor.window_buffer().point();
    assert_eq!(editor.window_buffer().contents(point..point + 1), "y");
    assert_eq!(editor.echo().as_deref(), Some("Also edited lib.rs"));
    let lib_buffer = editor.buffers().last().unwrap();
    assert_eq!(lib_buffer.path(), Some(lib.as_path()));
    assert!(lib_buffer.is_modified());
    assert_eq!(fs::read_to_string(&lib).unwrap(), "pub fn f() {}\n");

    press(&mut editor, "C-/");
    assert_eq!(contents(&editor), MAIN);
    assert_eq!(editor.window_buffer().path(), Some(main.as_path()));
}

#[test]
fn answers_after_another_command_are_dropped() {
    let (mut editor, main, _) = editor("late");
    press(&mut editor, "C-c C-a C-f");
    deliver(&mut editor, 1, vec![action("Fix", "quick fix", true)]);
    assert!(editor.popups().is_empty());

    press(&mut editor, "C-c C-a");
    deliver(&mut editor, 2, vec![action("Fix", "quick fix", true)]);
    press(&mut editor, "RET C-f");
    let changes = vec![FileEdits {
        path: main,
        edits: vec![(0..0, "x".into())],
    }];
    assert!(apply(&mut editor, 2, changes).is_err());
    failed(&mut editor, 2, "too late");
    assert_eq!(contents(&editor), MAIN);
    assert_eq!(editor.echo(), None);
}

#[test]
fn the_popup_closes_on_quit_and_without_actions() {
    let (mut editor, _, _) = editor("quit");
    press(&mut editor, "C-c C-a");
    deliver(&mut editor, 1, Vec::new());
    assert_eq!(editor.echo().as_deref(), Some("No code actions at point"));
    press(&mut editor, "C-c C-a");
    deliver(&mut editor, 2, vec![action("Fix", "", false)]);
    assert_eq!(rows(&editor), [("Fix".into(), None)]);
    press(&mut editor, "C-g");
    assert!(editor.popups().is_empty());
    assert_eq!(take_requests(&mut editor).len(), 2);
}

#[test]
fn fix_all_carries_out_the_only_fix_and_offers_several() {
    let (mut editor, _, _) = editor("fix-all");
    editor.run_command("fix-all");
    assert_eq!(
        take_requests(&mut editor),
        [Request::List {
            id: 1,
            range: 0..MAIN.len(),
            kind: Some("source.fixAll")
        }]
    );
    deliver(
        &mut editor,
        1,
        vec![action("Fix all oxlint", "fix all", false)],
    );
    assert!(editor.popups().is_empty());
    assert_eq!(
        take_requests(&mut editor),
        [Request::Apply { id: 1, index: 0 }]
    );

    editor.run_command("fix-all");
    deliver(&mut editor, 2, Vec::new());
    assert_eq!(editor.echo().as_deref(), Some("Nothing to fix"));
    editor.run_command("fix-all");
    let fixes = vec![
        action("Fix all oxlint", "", false),
        action("Fix all ts", "", false),
    ];
    deliver(&mut editor, 3, fixes);
    assert_eq!(rows(&editor).len(), 2);
}
