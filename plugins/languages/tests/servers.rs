//! Runs the real language servers, so it needs them installed: `cargo test -- --ignored`.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use remacs_core::{Editor, Icon, Plugin};
use remacs_diagnostics::DiagnosticsPlugin;
use remacs_languages::{LanguagesPlugin, SERVERS};
use remacs_lsp::LspPlugin;

static LSP: LspPlugin = LspPlugin::new(SERVERS);
const PLUGINS: &[&dyn Plugin] = &[&LanguagesPlugin, &DiagnosticsPlugin, &LSP];
const TIMEOUT: Duration = Duration::from_secs(120);

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("remacs-servers-{}/{name}", std::process::id()));
    for (path, contents) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    root
}

/// Severity and summary of each problem the list shows.
fn problems(editor: &mut Editor) -> Vec<(Icon, String)> {
    editor.refresh();
    assert_eq!(editor.echo(), None, "the server failed to start");
    editor.run_command("list-problems");
    let problems = editor
        .minibuffer()
        .map(|minibuffer| {
            minibuffer
                .candidates()
                .iter()
                .map(|candidate| (candidate.icon.unwrap(), candidate.label.clone()))
                .collect()
        })
        .unwrap_or_default();
    editor.cancel_minibuffer();
    problems
}

fn wait_until(
    editor: &mut Editor,
    done: impl Fn(&[(Icon, String)]) -> bool,
) -> Vec<(Icon, String)> {
    let started = Instant::now();
    while started.elapsed() < TIMEOUT {
        let problems = problems(editor);
        if done(&problems) {
            return problems;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("still {:?} after {TIMEOUT:?}", problems(editor));
}

fn check(file: &Path, expected: Icon) -> Editor {
    let mut editor = Editor::new(PLUGINS);
    editor.visit_file(file).unwrap();
    let problems = wait_until(&mut editor, |problems| !problems.is_empty());
    eprintln!("{}: {problems:?}", file.display());
    assert!(problems.iter().any(|(icon, _)| *icon == expected));
    editor
}

/// Replaces `wrong` with `right`, saves and waits for the problems to go away.
fn fix_and_save(editor: &mut Editor, wrong: &str, right: &str) {
    let buffer = editor.window_buffer_mut();
    let source = buffer.contents(0..buffer.text().len());
    let start = source.find(wrong).unwrap();
    buffer.delete(start..start + wrong.len());
    buffer.set_point(start);
    buffer.insert(right);
    editor.refresh();
    editor.window_buffer_mut().save().unwrap();
    wait_until(editor, |problems| problems.is_empty());
}

#[test]
#[ignore]
fn rust_analyzer_checks_on_open_and_save() {
    let root = project(
        "rust",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"demo\"\nedition = \"2024\"\n",
            ),
            ("src/main.rs", "fn main() {\n    let x: i32 = \"a\";\n}\n"),
        ],
    );
    let mut editor = check(&root.join("src/main.rs"), Icon::Error);
    fix_and_save(&mut editor, "let x: i32 = \"a\"", "let _x: i32 = 1");
}

#[test]
#[ignore]
fn clangd_reports_errors() {
    let root = project("c", &[("main.c", "int main(void) { return \"a\" + ; }\n")]);
    check(&root.join("main.c"), Icon::Error);
}

#[test]
#[ignore]
fn typescript_reports_type_errors() {
    let root = project(
        "ts",
        &[
            ("package.json", "{}\n"),
            ("index.ts", "const count: number = \"a\";\n"),
        ],
    );
    let mut editor = check(&root.join("index.ts"), Icon::Error);
    fix_and_save(&mut editor, "\"a\"", "1");
}

#[test]
#[ignore]
fn css_language_server_reports_unknown_properties() {
    let root = project("css", &[("style.css", "a { colr: red; }\n")]);
    check(&root.join("style.css"), Icon::Warning);
}
