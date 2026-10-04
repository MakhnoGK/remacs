//! Runs the real language servers, so it needs them installed: `cargo test -- --ignored`.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use remacs_completion::CompletionPlugin;
use remacs_core::{Editor, Icon, Plugin, Popup};
use remacs_diagnostics::DiagnosticsPlugin;
use remacs_languages::{LanguagesPlugin, SERVERS};
use remacs_lsp::LspPlugin;

static LSP: LspPlugin = LspPlugin::new(SERVERS);
const PLUGINS: &[&dyn Plugin] = &[
    &LanguagesPlugin,
    &DiagnosticsPlugin,
    &CompletionPlugin,
    &LSP,
];
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

/// Asks for completions after `before` until `wanted` is offered, selects and inserts it.
fn complete(file: &Path, before: &str, wanted: &str) -> String {
    let mut editor = Editor::new(PLUGINS);
    editor.visit_file(file).unwrap();
    let source = fs::read_to_string(file).unwrap();
    let point = source.find(before).unwrap() + before.len();
    editor.window_buffer_mut().set_point(point);
    let started = Instant::now();
    let offers = |popup: &Popup| {
        popup
            .rows
            .iter()
            .position(|row| row.label.starts_with(wanted))
    };
    let popup = loop {
        assert!(started.elapsed() < TIMEOUT, "{wanted} never offered");
        editor.run_command("completion-at-point");
        thread::sleep(Duration::from_millis(300));
        editor.refresh();
        if let Some(popup) = editor
            .popups()
            .pop()
            .filter(|popup| offers(popup).is_some())
        {
            break popup;
        }
    };
    for _ in 0..offers(&popup).unwrap() {
        editor.run_command("completion-next");
    }
    thread::sleep(Duration::from_millis(500));
    editor.refresh();
    let popup = editor.popups().pop().unwrap();
    let selected = &popup.rows[popup.selected.unwrap()];
    eprintln!(
        "{}: {} {:?} {:?}\n{:#?}",
        file.display(),
        selected.label,
        selected.kind,
        selected.detail,
        popup.documentation
    );
    editor.run_command("completion-insert");
    let buffer = editor.window_buffer();
    buffer.contents(0..buffer.text().len())
}

#[test]
#[ignore]
fn servers_complete_at_point() {
    let rust = project(
        "rust-completion",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"demo\"\nedition = \"2024\"\n",
            ),
            (
                "src/main.rs",
                "fn main() {\n    let range = 0..10usize;\n    let _ = range.st;\n}\n",
            ),
        ],
    );
    let typescript = project(
        "ts-completion",
        &[
            ("package.json", "{}\n"),
            ("index.ts", "const s = \"a\";\ns.toUpp\n"),
        ],
    );
    let c = project(
        "c-completion",
        &[(
            "main.c",
            "#include <string.h>\nint main(void) { return strle; }\n",
        )],
    );
    let css = project("css-completion", &[("style.css", "a { colo }\n")]);
    let html = project(
        "html-completion",
        &[("index.html", "<body><secti</body>\n")],
    );
    let cases = [
        (
            rust.join("src/main.rs"),
            "range.st",
            "start",
            "range.start;",
        ),
        (
            rust.join("src/main.rs"),
            "range.st",
            "step_by",
            "range.step_by();",
        ),
        (
            typescript.join("index.ts"),
            "s.toUpp",
            "toUpperCase",
            "s.toUpperCase",
        ),
        (c.join("main.c"), "strle", "strlen", "return strlen("),
        (css.join("style.css"), "colo", "color", "a { color: "),
        (html.join("index.html"), "<secti", "section", "<section"),
    ];
    for (file, before, wanted, inserted) in cases {
        let text = complete(&file, before, wanted);
        eprintln!("{text}");
        assert!(text.contains(inserted), "{text}");
    }
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
