//! Runs the real language servers, so it needs them installed: `cargo test -- --ignored`.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use remacs_actions::ActionsPlugin;
use remacs_completion::CompletionPlugin;
use remacs_core::{Documentation, Editor, Icon, Plugin, Popup};
use remacs_diagnostics::DiagnosticsPlugin;
use remacs_documentation::DocumentationPlugin;
use remacs_languages::{LanguagesPlugin, SERVERS};
use remacs_lsp::LspPlugin;
use remacs_xref::XrefPlugin;

static LSP: LspPlugin = LspPlugin::new(SERVERS);
const PLUGINS: &[&dyn Plugin] = &[
    &LanguagesPlugin,
    &DiagnosticsPlugin,
    &CompletionPlugin,
    &DocumentationPlugin,
    &XrefPlugin,
    &ActionsPlugin,
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

/// Runs `command` with point in `symbol` until `answered` finds what it wanted.
fn ask_at<T>(
    file: &Path,
    symbol: &str,
    command: &str,
    answered: impl Fn(&Editor) -> Option<T>,
) -> T {
    let mut editor = Editor::new(PLUGINS);
    editor.visit_file(file).unwrap();
    let source = fs::read_to_string(file).unwrap();
    let point = source.find(symbol).unwrap() + 1;
    let started = Instant::now();
    loop {
        assert!(
            started.elapsed() < TIMEOUT,
            "{command} on {symbol} never answered"
        );
        editor.visit_file(file).unwrap();
        editor.window_buffer_mut().set_point(point);
        editor.run_command(command);
        thread::sleep(Duration::from_millis(300));
        editor.refresh();
        if let Some(answer) = answered(&editor) {
            return answer;
        }
    }
}

fn documentation(file: &Path, symbol: &str) -> Documentation {
    let documentation = ask_at(file, symbol, "show-documentation", |editor| {
        editor.popups().pop()?.documentation
    });
    eprintln!("{}: {symbol}: {documentation:#?}", file.display());
    documentation
}

/// The file and line point lands on from `call`.
fn definition(file: &Path, call: &str) -> (PathBuf, String) {
    ask_at(file, call, "xref-find-definitions", |editor| {
        let buffer = editor.window_buffer();
        let range = buffer.text().line_range(buffer.point_line())?;
        let line = buffer.contents(range).trim().to_owned();
        (!line.contains(call)).then(|| (buffer.path().unwrap().to_path_buf(), line))
    })
}

#[test]
#[ignore]
fn servers_show_documentation_and_go_to_definitions() {
    let rust = project(
        "rust-navigation",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"demo\"\nedition = \"2024\"\n",
            ),
            (
                "src/main.rs",
                "mod util;\n\nfn main() {\n    let mut bytes = [0u8; 2];\n    bytes.copy_from_slice(&[1, 2]);\n    util::helper();\n}\n",
            ),
            ("src/util.rs", "/// Says hello.\npub fn helper() {}\n"),
        ],
    );
    let typescript = project(
        "ts-navigation",
        &[
            ("package.json", "{}\n"),
            (
                "index.ts",
                "import { helper } from \"./util\";\n\"a\".toUpperCase();\nhelper();\n",
            ),
            (
                "util.ts",
                "/** Says hello. */\nexport function helper() {}\n",
            ),
        ],
    );
    let c = project(
        "c-navigation",
        &[(
            "main.c",
            "#include <string.h>\n\nstatic int helper(void) { return 0; }\n\nint main(void) { return helper() + strlen(\"a\"); }\n",
        )],
    );
    let css = project("css-navigation", &[("style.css", "a { color: red; }\n")]);
    let html = project(
        "html-navigation",
        &[("index.html", "<body><section></section></body>\n")],
    );

    let copy = documentation(&rust.join("src/main.rs"), "copy_from_slice");
    assert_eq!(copy.kind, "method");
    let signature = copy.signature.unwrap();
    assert!(signature.text.contains("fn copy_from_slice"));
    assert_eq!(signature.mode, Some("Rust"));
    assert!(!copy.blocks.is_empty());
    let upper = documentation(&typescript.join("index.ts"), "toUpperCase");
    assert_eq!(upper.kind, "method");
    let strlen = documentation(&c.join("main.c"), "strlen");
    assert!(!strlen.is_empty());
    assert!(!documentation(&css.join("style.css"), "color").is_empty());
    assert!(!documentation(&html.join("index.html"), "section").is_empty());

    let cases = [
        (rust.join("src/main.rs"), "helper();", "pub fn helper() {}"),
        (
            typescript.join("index.ts"),
            "helper();",
            "export function helper() {}",
        ),
        (
            c.join("main.c"),
            "helper() +",
            "static int helper(void) { return 0; }",
        ),
    ];
    for (file, call, expected) in cases {
        let (path, line) = definition(&file, call);
        eprintln!("{}: {line}", path.display());
        assert_eq!(line, expected);
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

/// Asks for code actions at `at` until `wanted` is offered, applies it and returns the
/// buffer once it changed.
fn apply_action(file: &Path, at: &str, wanted: &str) -> String {
    let mut editor = Editor::new(PLUGINS);
    editor.visit_file(file).unwrap();
    let source = fs::read_to_string(file).unwrap();
    let point = source.find(at).unwrap();
    let started = Instant::now();
    let offered = |popup: &Popup| popup.rows.iter().position(|row| row.label.contains(wanted));
    let index = loop {
        assert!(started.elapsed() < TIMEOUT, "{wanted} never offered");
        editor.window_buffer_mut().set_point(point);
        editor.run_command("code-actions");
        thread::sleep(Duration::from_millis(300));
        editor.refresh();
        if let Some(popup) = editor.popups().pop() {
            let titles: Vec<_> = popup.rows.iter().map(|row| row.label.as_str()).collect();
            eprintln!("{}: {titles:?}", file.display());
            if let Some(index) = offered(&popup) {
                break index;
            }
        }
    };
    for _ in 0..index {
        editor.run_command("code-actions-next");
    }
    editor.run_command("code-actions-apply");
    let contents = |editor: &Editor| {
        let buffer = editor.window_buffer();
        buffer.contents(0..buffer.text().len())
    };
    while contents(&editor) == source {
        assert!(started.elapsed() < TIMEOUT, "{wanted} never applied");
        let echo = editor.echo().unwrap_or_default();
        assert!(!echo.starts_with("Code action"), "{echo}");
        thread::sleep(Duration::from_millis(100));
        editor.refresh();
    }
    contents(&editor)
}

#[test]
#[ignore]
fn servers_offer_and_apply_code_actions() {
    let rust = project(
        "rust-actions",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"demo\"\nedition = \"2024\"\n",
            ),
            (
                "src/main.rs",
                "fn main() {\n    let x = 1;\n    let _ = x;\n}\n",
            ),
        ],
    );
    let c = project("c-actions", &[("main.c", "int main(void) { return 0 }\n")]);
    let text = apply_action(&rust.join("src/main.rs"), "x = 1", "explicit type");
    assert!(text.contains("let x: i32 = 1;"), "{text}");
    let text = apply_action(&c.join("main.c"), "}", "insert ';'");
    assert!(text.contains("return 0; }"), "{text}");
    let typescript = project(
        "ts-actions",
        &[
            ("package.json", "{}\n"),
            ("tsconfig.json", "{}\n"),
            ("index.ts", "helper();\n"),
            ("util.ts", "export function helper() {}\n"),
        ],
    );
    let text = apply_action(&typescript.join("index.ts"), "helper", "import");
    assert!(text.contains("import { helper } from"), "{text}");
}
