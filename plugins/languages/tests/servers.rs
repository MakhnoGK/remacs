//! Runs the real language servers, so it needs them installed: `cargo test -- --ignored`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use remacs_actions::ActionsPlugin;
use remacs_completion::CompletionPlugin;
use remacs_core::{
    Changes, Documentation, Editor, Elsewhere, Icon, Plugin, Popup, Settings, parse_sequence,
};
use remacs_diagnostics::DiagnosticsPlugin;
use remacs_dired::{DiredPlugin, list_directory};
use remacs_documentation::DocumentationPlugin;
use remacs_editing::EditingPlugin;
use remacs_format::FormatPlugin;
use remacs_languages::{GRAMMARS, INJECTIONS, LanguagesPlugin, PRETTIER_MODES, SERVERS};
use remacs_lsp::LspPlugin;
use remacs_prettier::PrettierPlugin;
use remacs_rename::RenamePlugin;
use remacs_syntax::SyntaxPlugin;
use remacs_xref::XrefPlugin;

static LSP: LspPlugin = LspPlugin::new(SERVERS);
static PRETTIER: PrettierPlugin = PrettierPlugin::new(PRETTIER_MODES);
// Tells the code embedded in HTML, which goes to its languages' servers.
static SYNTAX: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, INJECTIONS);
const PLUGINS: &[&dyn Plugin] = &[
    &LanguagesPlugin,
    &SYNTAX,
    &DiagnosticsPlugin,
    &CompletionPlugin,
    &DocumentationPlugin,
    &XrefPlugin,
    &ActionsPlugin,
    &FormatPlugin,
    &PRETTIER,
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
        editor.cancel_minibuffer();
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

/// The file and line text of each use of `symbol` listed, once there are `count`.
fn references(file: &Path, symbol: &str, count: usize) -> Vec<(String, String)> {
    let references = ask_at(file, symbol, "xref-find-references", |editor| {
        let rows: Vec<_> = (editor.minibuffer()?.candidates().iter())
            .map(|row| {
                (
                    row.group.as_deref().unwrap_or_default().to_owned(),
                    row.label.clone(),
                )
            })
            .collect();
        (rows.len() >= count).then_some(rows)
    });
    eprintln!("{}: {symbol}: {references:#?}", file.display());
    references
}

/// The signature shown with point after the first character of `at`, and the parameter
/// in it that point is in.
fn signature_at(editor: &Editor) -> Option<(String, String)> {
    let popup = editor.popups().into_iter().find(|popup| popup.above)?;
    let label = popup.documentation?.signature?;
    let parameter = label.text[label.emphasis.clone()?].to_owned();
    Some((label.text, parameter))
}

fn signature(file: &Path, at: &str) -> (String, String) {
    let signature = ask_at(file, at, "signature-help", signature_at);
    eprintln!("{}: {at}: {signature:?}", file.display());
    signature
}

#[test]
#[ignore]
fn servers_find_references_and_show_signatures() {
    let rust = project(
        "rust-references",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"demo\"\nedition = \"2024\"\n",
            ),
            (
                "src/main.rs",
                "mod util;\n\nfn main() {\n    util::add(1, 2);\n}\n",
            ),
            (
                "src/util.rs",
                "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
            ),
        ],
    );
    let typescript = project(
        "ts-references",
        &[
            ("package.json", "{}\n"),
            (
                "index.ts",
                "import { add } from \"./util\";\nadd(1, 2);\nadd\n",
            ),
            (
                "util.ts",
                "export function add(a: number, b: number) {\n  return a + b;\n}\n",
            ),
        ],
    );
    let html = project(
        "html-references",
        &[(
            "index.html",
            "<body>\n<script>\nfunction add(a, b) { return a + b; }\nadd(1, 2);\n</script>\n</body>\n",
        )],
    );

    let main = rust.join("src/main.rs");
    let uses = references(&main, "add(1", 2);
    assert!(uses.contains(&("main.rs  ·  src/".into(), "util::add(1, 2);".into())));
    assert!(uses.contains(&(
        "util.rs  ·  src/".into(),
        "pub fn add(a: i32, b: i32) -> i32 {".into()
    )));
    let (label, parameter) = signature(&main, ", 2)");
    assert!(label.contains("fn add(a: i32, b: i32)"), "{label}");
    assert_eq!(parameter, "b: i32");

    let index = typescript.join("index.ts");
    let uses = references(&index, "add(1", 4);
    assert!(
        uses.iter()
            .any(|(file, line)| file.starts_with("util.ts") && line.contains("function add"))
    );
    assert_eq!(signature(&index, ", 2)").1, "b: number");

    let page = html.join("index.html");
    let uses = references(&page, "add(1", 2);
    assert!(
        uses.iter().all(|(file, _)| file.starts_with("index.html")),
        "{uses:?}"
    );
    assert_eq!(signature(&page, ", 2)").1, "b: any");

    // Typing a call opens its signature and follows the arguments, once the server answers.
    let typing: Vec<_> = PLUGINS
        .iter()
        .copied()
        .chain([&EditingPlugin as _])
        .collect();
    let mut editor = Editor::new(&typing);
    editor.visit_file(&index).unwrap();
    let source = fs::read_to_string(&index).unwrap();
    let press = |editor: &mut Editor, keys: &str| {
        for key in parse_sequence(keys).unwrap() {
            editor.handle_key(key);
        }
    };
    let shown = |editor: &mut Editor, parameter: &str| {
        let started = Instant::now();
        while signature_at(editor).is_none_or(|(_, shown)| shown != parameter) {
            assert!(
                started.elapsed() < TIMEOUT,
                "{parameter} never shown: {:?} {:?}",
                signature_at(editor),
                editor.echo()
            );
            thread::sleep(Duration::from_millis(100));
            editor.refresh();
            if editor.echo().as_deref() == Some("No signature at point") {
                editor.run_command("signature-help");
            }
        }
    };
    editor
        .window_buffer_mut()
        .set_point(source.find("1, 2").unwrap());
    editor.run_command("signature-help");
    shown(&mut editor, "a: number");
    press(&mut editor, "ESC");
    editor.window_buffer_mut().set_point(source.len() - 1);
    press(&mut editor, "(");
    shown(&mut editor, "a: number");
    press(&mut editor, "1 ,");
    shown(&mut editor, "b: number");

    // Off, typing leaves signatures alone while ⌘P still shows them.
    let lsp_toml = (
        PathBuf::from("lsp.toml"),
        "signature-help-on-typing = false\n".to_owned(),
    );
    editor.set_settings(Settings::from_files([lsp_toml]));
    press(&mut editor, "ESC");
    let end = editor.window_buffer().text().len();
    editor.window_buffer_mut().set_point(end);
    press(&mut editor, "a d d (");
    for _ in 0..10 {
        thread::sleep(Duration::from_millis(100));
        editor.refresh();
    }
    assert_eq!(signature_at(&editor), None);
    editor.run_command("signature-help");
    shown(&mut editor, "a: number");
}

/// Wakes of the editor, as the app's refreshes, and how long the refreshes took, while
/// typing a line of identifiers at a person's pace into a TypeScript file.
fn wakes_while_typing(root: &Path, signature_help: bool) -> (usize, Duration) {
    let typing: Vec<_> = PLUGINS
        .iter()
        .copied()
        .chain([&EditingPlugin as _])
        .collect();
    let mut editor = Editor::new(&typing);
    let lsp_toml = format!("signature-help-on-typing = {signature_help}\n");
    editor.set_settings(Settings::from_files([(
        PathBuf::from("lsp.toml"),
        lsp_toml,
    )]));
    let wakes = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&wakes);
    editor.set_notifier(Arc::new(move || {
        counted.fetch_add(1, Ordering::Relaxed);
    }));
    editor.visit_file(&root.join("index.ts")).unwrap();
    // Loaded once the first question is answered.
    documentation(&root.join("index.ts"), "add(1");
    editor.run_command("show-documentation");
    let started = Instant::now();
    while editor.popups().is_empty() && started.elapsed() < TIMEOUT {
        thread::sleep(Duration::from_millis(100));
        editor.refresh();
        editor.run_command("show-documentation");
    }
    editor.run_command("hide-documentation");
    let end = editor.window_buffer().text().len();
    editor.window_buffer_mut().set_point(end);
    thread::sleep(Duration::from_secs(1));
    editor.refresh();
    wakes.store(0, Ordering::Relaxed);
    let mut refreshing = Duration::ZERO;
    let mut refreshes = 0;
    for character in "let total = count + offset * scale".chars() {
        let key = if character == ' ' {
            "SPC".to_owned()
        } else {
            character.to_string()
        };
        for key in parse_sequence(&key).unwrap() {
            editor.handle_key(key);
        }
        // About 8 keys a second; each wake refreshes, as the app does.
        let typed = Instant::now();
        while typed.elapsed() < Duration::from_millis(120) {
            if wakes.load(Ordering::Relaxed) > refreshes {
                refreshes = wakes.load(Ordering::Relaxed);
                let refresh = Instant::now();
                editor.refresh();
                refreshing += refresh.elapsed();
            }
            thread::sleep(Duration::from_millis(2));
        }
    }
    (wakes.load(Ordering::Relaxed), refreshing)
}

/// Keys that open no call are answered without the server's threads, so signature help
/// wakes the editor no more often while typing; answered by them, it was once per key.
#[test]
#[ignore]
fn signature_help_wakes_the_editor_no_more_often_while_typing() {
    let typescript = project(
        "ts-typing-wakes",
        &[
            ("package.json", "{}\n"),
            (
                "index.ts",
                "function add(a: number, b: number) {\n  return a + b;\n}\nadd(1, 2);\n",
            ),
        ],
    );
    let mut wakes = [0; 2];
    for signature_help in [false, true, false, true] {
        let (woken, refreshing) = wakes_while_typing(&typescript, signature_help);
        eprintln!(
            "signature help on typing {signature_help}: {woken} wakes, refreshes took {refreshing:?}"
        );
        wakes[usize::from(signature_help)] += woken;
    }
    let [off, on] = wakes;
    // Typed 35 keys twice; servers' own messages vary by a few.
    assert!(
        on <= off + 10,
        "{on} wakes with signature help, {off} without"
    );
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

const LINTED: &str = "const count: number = \"a\";\ndebugger;\n";

#[test]
#[ignore]
fn oxlint_checks_beside_typescript_with_the_projects_own_copy() {
    // Runs the installed oxlint, leaving a mark that the project's copy was chosen.
    let root = project(
        "oxlint",
        &[
            (
                "package.json",
                "{ \"devDependencies\": { \"oxlint\": \"1\" } }\n",
            ),
            ("index.ts", LINTED),
            (
                "node_modules/.bin/oxlint",
                "#!/bin/sh\ntouch \"$(dirname \"$0\")/started\"\nexec oxlint \"$@\"\n",
            ),
        ],
    );
    let program = root.join("node_modules/.bin/oxlint");
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    let mut editor = Editor::new(PLUGINS);
    editor.visit_file(&root.join("index.ts")).unwrap();
    let problems = wait_until(&mut editor, |problems| {
        let says = |text: &str| problems.iter().any(|(_, label)| label.contains(text));
        says("not assignable") && says("debugger")
    });
    eprintln!("{problems:?}");
    assert!(root.join("node_modules/.bin/started").exists());
    let text = apply_action(&root.join("index.ts"), "debugger", "Remove the debugger");
    assert_eq!(text, "const count: number = \"a\";\n\n");
}

#[test]
#[ignore]
fn vite_plus_projects_lint_and_format_with_vite_config() {
    // Needs the oxlint and oxfmt of an installed vite-plus first on PATH: copies of their
    // own cannot load a vite.config.ts.
    let root = project(
        "vite-plus",
        &[
            (
                "package.json",
                "{ \"devDependencies\": { \"vite-plus\": \"1\" } }\n",
            ),
            (
                "vite.config.ts",
                "export default {\n  lint: { rules: { eqeqeq: 'error', 'no-useless-rename': 'error' } },\n  fmt: { semi: false, singleQuote: true },\n}\n",
            ),
            ("index.ts", "if (1 == 2) {}\nconst a={b:\"c\"};\n"),
        ],
    );
    let mut editor = Editor::new(PLUGINS);
    editor.visit_file(&root.join("index.ts")).unwrap();
    let problems = wait_until(&mut editor, |problems| {
        problems.iter().any(|(_, label)| label.contains("==="))
    });
    eprintln!("{problems:?}");
    assert_eq!(
        format(&root.join("index.ts")),
        "if (1 == 2) {\n}\nconst a = { b: 'c' }\n"
    );

    let fixed = "const { c } = { c: 1 };\nexport { c };\n";
    fs::write(
        root.join("fix.ts"),
        "const { c: c } = { c: 1 };\nexport { c };\n",
    )
    .unwrap();
    let mut editor = Editor::new(PLUGINS);
    editor.visit_file(&root.join("fix.ts")).unwrap();
    wait_until(&mut editor, |problems| {
        problems.iter().any(|(_, label)| label.contains("renam"))
    });
    editor.run_command("fix-all");
    refresh_until(&mut editor, "the fixes", |editor| {
        let buffer = editor.window_buffer();
        buffer.contents(0..buffer.text().len()) == fixed
    });
}

#[test]
#[ignore]
fn linters_leave_projects_without_their_config_alone() {
    let root = project(
        "no-linters",
        &[("package.json", "{}\n"), ("index.ts", LINTED)],
    );
    let mut editor = check(&root.join("index.ts"), Icon::Error);
    thread::sleep(Duration::from_secs(2));
    let problems = problems(&mut editor);
    assert!(
        problems
            .iter()
            .all(|(_, label)| !label.contains("debugger")),
        "{problems:?}"
    );
}

#[test]
#[ignore]
fn eslint_checks_and_fixes_with_the_projects_config() {
    let root = project(
        "eslint",
        &[
            ("package.json", "{ \"type\": \"module\" }\n"),
            (
                "eslint.config.js",
                "export default [{ rules: { \"prefer-const\": \"error\" } }];\n",
            ),
            ("index.js", "let answer = 42;\nconsole.log(answer);\n"),
        ],
    );
    let mut editor = Editor::new(PLUGINS);
    editor.visit_file(&root.join("index.js")).unwrap();
    let problems = wait_until(&mut editor, |problems| {
        problems
            .iter()
            .any(|(_, label)| label.contains("never reassigned"))
    });
    eprintln!("{problems:?}");
    let text = apply_action(&root.join("index.js"), "answer", "Fix this prefer-const");
    assert_eq!(text, "const answer = 42;\nconsole.log(answer);\n");
}

/// Refreshes until `done`, as the listing and the server's answers arrive.
fn refresh_until(editor: &mut Editor, what: &str, done: impl Fn(&Editor) -> bool) {
    let started = Instant::now();
    while !done(editor) {
        assert!(started.elapsed() < TIMEOUT, "{what} never happened");
        thread::sleep(Duration::from_millis(100));
        editor.refresh();
    }
}

#[test]
#[ignore]
fn rust_analyzer_updates_mod_declarations_of_files_renamed_in_dired() {
    let root = project(
        "rust-rename",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"demo\"\nedition = \"2024\"\n",
            ),
            ("src/lib.rs", "mod casefiddle;\n\npub use casefiddle::up;\n"),
            ("src/casefiddle.rs", "pub fn up() {}\n"),
        ],
    );
    let src = root.join("src").canonicalize().unwrap();
    let plugins: Vec<&dyn Plugin> = [&DiredPlugin as &dyn Plugin]
        .into_iter()
        .chain(PLUGINS.iter().copied())
        .collect();
    let mut editor = Editor::new(&plugins);
    list_directory(&mut editor, &src).unwrap();
    refresh_until(&mut editor, "listing", |editor| {
        editor.window_buffer().text().line_count() > 1
    });
    editor.run_command("wdired-change-to-wdired-mode");
    let buffer = editor.window_buffer_mut();
    let source = buffer.contents(0..buffer.text().len());
    buffer.set_point(source.find("casefiddle").unwrap() + "case".len());
    buffer.insert("_");
    editor.refresh();

    refresh_until(&mut editor, "the mod update", |editor| {
        let changes = editor.table(0..10).and_then(|table| table.changes);
        matches!(changes, Some(Changes { elsewhere: Elsewhere::Files(files), .. }) if !files.is_empty())
    });
    editor.run_command("wdired-finish-edit");
    refresh_until(&mut editor, "the rename", |editor| {
        editor
            .echo()
            .is_some_and(|echo| echo.starts_with("Renamed"))
    });
    eprintln!("{}", editor.echo().unwrap());
    let lib = fs::read_to_string(src.join("lib.rs")).unwrap();
    assert!(lib.starts_with("mod case_fiddle;"), "{lib}");
    assert!(src.join("case_fiddle.rs").exists());
}

/// Renames the symbol `at` points into to `new_name`, asking again while the server is
/// still loading the project; returns the editor once the rename is said to be done.
fn rename_symbol(file: &Path, at: &str, new_name: &str) -> Editor {
    let plugins: Vec<&dyn Plugin> = PLUGINS
        .iter()
        .copied()
        .chain([&RenamePlugin as &dyn Plugin])
        .collect();
    let mut editor = Editor::new(&plugins);
    editor.visit_file(file).unwrap();
    let source = fs::read_to_string(file).unwrap();
    let point = source.find(at).unwrap() + 1;
    let started = Instant::now();
    loop {
        assert!(started.elapsed() < TIMEOUT, "never asked for the new name");
        editor.window_buffer_mut().set_point(point);
        editor.run_command("rename-symbol");
        thread::sleep(Duration::from_millis(300));
        editor.refresh();
        if editor.minibuffer().is_some() {
            break;
        }
        eprintln!("{:?}", editor.echo());
    }
    let minibuffer = editor.minibuffer_mut().unwrap();
    eprintln!("offered {:?}", minibuffer.input_text());
    minibuffer.set_input(new_name);
    editor.exit_minibuffer(false).unwrap();
    refresh_until(&mut editor, "the rename", |editor| {
        editor
            .echo()
            .is_some_and(|echo| !echo.starts_with("Renaming"))
    });
    eprintln!("{}", editor.echo().unwrap());
    editor
}

#[test]
#[ignore]
fn rust_analyzer_renames_symbols_everywhere_saving_every_file() {
    let root = project(
        "rust-rename-symbol",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"demo\"\nedition = \"2024\"\n",
            ),
            (
                "src/lib.rs",
                "mod util;\n\npub fn run() {\n    util::helper();\n}\n",
            ),
            ("src/util.rs", "pub fn helper() {}\n"),
        ],
    );
    let src = root.join("src").canonicalize().unwrap();
    let editor = rename_symbol(&src.join("lib.rs"), "helper();", "assist");
    assert!(editor.echo().unwrap().ends_with("2 places in 2 files"));
    assert!(!editor.window_buffer().is_modified());
    let lib = fs::read_to_string(src.join("lib.rs")).unwrap();
    assert!(lib.contains("util::assist();"), "{lib}");
    let util = fs::read_to_string(src.join("util.rs")).unwrap();
    assert_eq!(util, "pub fn assist() {}\n");
}

#[test]
#[ignore]
fn rust_analyzer_renames_modules_with_their_files() {
    let root = project(
        "rust-rename-module",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"demo\"\nedition = \"2024\"\n",
            ),
            (
                "src/lib.rs",
                "mod util;\n\npub fn run() {\n    util::assist();\n}\n",
            ),
            ("src/util.rs", "pub fn assist() {}\n"),
        ],
    );
    let src = root.join("src").canonicalize().unwrap();
    let editor = rename_symbol(&src.join("lib.rs"), "util::", "tools");
    assert!(editor.echo().unwrap().contains("moved util.rs to tools.rs"));
    let lib = fs::read_to_string(src.join("lib.rs")).unwrap();
    assert!(
        lib.starts_with("mod tools;") && lib.contains("tools::assist();"),
        "{lib}"
    );
    assert!(src.join("tools.rs").exists() && !src.join("util.rs").exists());
}

#[test]
#[ignore]
fn typescript_updates_imports_of_files_moved_in_dired() {
    let root = project(
        "ts-move",
        &[
            ("tsconfig.json", "{}\n"),
            ("src/a.ts", "export const x = 1;\n"),
            (
                "src/b.ts",
                "import { x } from \"./a\";\n\nconsole.log(x);\n",
            ),
        ],
    );
    let src = root.join("src").canonicalize().unwrap();
    fs::create_dir_all(src.join("lib")).unwrap();
    let plugins: Vec<&dyn Plugin> = [&DiredPlugin as &dyn Plugin]
        .into_iter()
        .chain(PLUGINS.iter().copied())
        .collect();
    let mut editor = Editor::new(&plugins);
    // A served file starts the server for the project.
    editor.visit_file(&src.join("b.ts")).unwrap();
    list_directory(&mut editor, &src).unwrap();
    refresh_until(&mut editor, "listing", |editor| {
        editor.window_buffer().text().line_count() > 2
    });
    let buffer = editor.window_buffer_mut();
    let source = buffer.contents(0..buffer.text().len());
    buffer.set_point(source.find("a.ts").unwrap());
    editor.run_command("dired-do-rename");
    editor.minibuffer_mut().unwrap().set_input("lib/");
    editor.exit_minibuffer(true).unwrap();
    refresh_until(&mut editor, "the move", |editor| {
        editor.echo().is_some_and(|echo| echo.starts_with("Moved"))
    });
    eprintln!("{}", editor.echo().unwrap());
    let b = fs::read_to_string(src.join("b.ts")).unwrap();
    assert!(b.starts_with("import { x } from \"./lib/a\";"), "{b}");
    assert!(src.join("lib/a.ts").exists());
}

/// Formats the whole file and returns the buffer once it changed.
fn format(file: &Path) -> String {
    format_region(file, None)
}

/// Formats `region` of the file, all of it without one, and returns the buffer once it
/// changed. Code of another language in the region is waited for until it is told.
fn format_region(file: &Path, region: Option<(&str, &str)>) -> String {
    let mut editor = Editor::new(PLUGINS);
    editor.visit_file(file).unwrap();
    let source = fs::read_to_string(file).unwrap();
    match region {
        Some((region, mode)) => {
            let start = source.find(region).unwrap();
            refresh_until(&mut editor, "the embedded code", |editor| {
                editor.language_at(editor.window_buffer(), start) == mode
            });
            let buffer = editor.window_buffer_mut();
            buffer.set_mark(start);
            buffer.activate_mark();
            buffer.set_point(start + region.len());
            editor.run_command("format");
        }
        None => editor.run_command("format-buffer"),
    }
    let contents = |editor: &Editor| {
        let buffer = editor.window_buffer();
        buffer.contents(0..buffer.text().len())
    };
    refresh_until(&mut editor, "formatting", |editor| {
        let echo = editor.echo();
        let waiting = echo.as_ref().is_none_or(|echo| echo.starts_with("Waiting"));
        assert!(waiting, "{}: {echo:?}", file.display());
        contents(editor) != source
    });
    contents(&editor)
}

#[test]
#[ignore]
fn language_servers_format_files() {
    let rust = project(
        "rust-format",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"demo\"\nedition = \"2024\"\n",
            ),
            ("src/main.rs", "fn main(){let x=1;println!(\"{x}\");}\n"),
        ],
    );
    assert_eq!(
        format(&rust.join("src/main.rs")),
        "fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}\n"
    );
    // Formatted before rust-analyzer loads the project, rustfmt would miss the crate's
    // edition and sort imports in the 2015 style.
    let configured = project(
        "rustfmt-toml",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"demo\"\nedition = \"2024\"\n",
            ),
            ("rustfmt.toml", "hard_tabs = true\n"),
            (
                "src/main.rs",
                "use a::b::C;\nuse a::D;\nfn main(){let x=1;}\n",
            ),
        ],
    );
    assert_eq!(
        format(&configured.join("src/main.rs")),
        "use a::D;\nuse a::b::C;\nfn main() {\n\tlet x = 1;\n}\n"
    );
    let c = project("c-format", &[("main.c", "int main(void){return 0;}\n")]);
    assert_eq!(format(&c.join("main.c")), "int main(void) { return 0; }\n");
    let typescript = project(
        "ts-format",
        &[("package.json", "{}\n"), ("index.ts", "const a={b:1}\n")],
    );
    assert_eq!(format(&typescript.join("index.ts")), "const a = { b: 1 }\n");
    let css = project("css-format", &[("style.css", "a{color:red}\n")]);
    assert_eq!(format(&css.join("style.css")), "a {\n    color: red\n}\n");
}

#[test]
#[ignore]
fn prettier_formats_projects_that_use_it_with_their_config() {
    let root = project(
        "prettier",
        &[
            ("package.json", "{}\n"),
            (
                ".prettierrc",
                "{ \"semi\": false, \"singleQuote\": true }\n",
            ),
            ("index.ts", "const a={b:\"c\"};\n"),
            ("broken.ts", "const = ;\n"),
            ("style.css", "a{color:red}\n"),
            ("rules.css", "a{color:red}\nb{color:blue}\n"),
            ("blank.css", "a{color:red}\n\nb{color:blue}\n"),
            (
                "page.html",
                "<script>\nconst a={b:\"c\"};\n</script>\n<p>x</p>\n",
            ),
            (
                "styled.html",
                "<style>\n  a{\n    /* x\n       y */\n    color:red}\n  b{color:blue}\n</style>\n",
            ),
        ],
    );
    assert_eq!(format(&root.join("index.ts")), "const a = { b: 'c' }\n");
    assert_eq!(format(&root.join("style.css")), "a {\n  color: red;\n}\n");
    // A region of CSS formats the rules it touches, in a file of its own or not (#69); a
    // region ending where the next rule starts leaves it be.
    assert_eq!(
        format_region(&root.join("rules.css"), Some(("a{color:red}\n", "CSS"))),
        "a {\n  color: red;\n}\nb{color:blue}\n"
    );
    assert_eq!(
        format_region(&root.join("styled.html"), Some(("a{", "CSS"))),
        "<style>\n  a {\n    /* x\n       y */\n    color: red;\n  }\n  b{color:blue}\n</style>\n"
    );
    let script = format_region(&root.join("page.html"), Some(("const a", "JavaScript")));
    assert_eq!(
        script,
        "<script>\nconst a = { b: 'c' }\n</script>\n<p>x</p>\n"
    );

    let mut editor = Editor::new(PLUGINS);
    editor.visit_file(&root.join("broken.ts")).unwrap();
    editor.run_command("format-buffer");
    refresh_until(&mut editor, "the failure", |editor| editor.echo().is_some());
    let echo = editor.echo().unwrap();
    assert!(echo.starts_with("prettier: "), "{echo}");

    // A region of CSS between rules says why nothing changed.
    let mut editor = Editor::new(PLUGINS);
    editor.visit_file(&root.join("blank.css")).unwrap();
    let buffer = editor.window_buffer_mut();
    buffer.set_mark(13);
    buffer.activate_mark();
    buffer.set_point(14);
    editor.run_command("format");
    refresh_until(&mut editor, "the failure", |editor| editor.echo().is_some());
    assert_eq!(
        editor.echo().as_deref(),
        Some("prettier: no rules in the region")
    );
}

/// The rules of a CSS region are formatted alone, but not as a file of their own.
#[test]
#[ignore]
fn prettier_adds_no_pragma_to_a_region_of_css() {
    let root = project(
        "prettier-pragma",
        &[
            ("package.json", "{}\n"),
            (".prettierrc", "{ \"insertPragma\": true }\n"),
            ("rules.css", "a{color:red}\nb{color:blue}\n"),
        ],
    );
    assert_eq!(
        format_region(&root.join("rules.css"), Some(("b{color:blue}", "CSS"))),
        "a{color:red}\nb {\n  color: blue;\n}\n"
    );
}

#[test]
#[ignore]
fn prettier_of_the_project_comes_before_the_global_one() {
    let own = "module.exports = {\n  getFileInfo: async () => ({ ignored: false, inferredParser: \"babel\" }),\n  resolveConfig: async () => null,\n  format: async () => \"// the project's own\\n\",\n};\n";
    let root = project(
        "own-prettier",
        &[
            ("package.json", "{}\n"),
            ("index.js", "let a\n"),
            (
                "node_modules/prettier/package.json",
                "{ \"main\": \"index.js\" }\n",
            ),
            ("node_modules/prettier/index.js", own),
        ],
    );
    assert_eq!(format(&root.join("index.js")), "// the project's own\n");
}

#[test]
#[ignore]
fn prettier_finds_the_plugins_of_the_projects_config() {
    let root = project(
        "prettier-plugin",
        &[
            ("package.json", "{}\n"),
            (
                ".prettierrc",
                "{ \"plugins\": [\"prettier-plugin-nothing\"] }\n",
            ),
            ("index.ts", "const a={b:1}\n"),
            (
                "node_modules/prettier-plugin-nothing/package.json",
                "{ \"main\": \"index.js\" }\n",
            ),
            (
                "node_modules/prettier-plugin-nothing/index.js",
                "module.exports = {};\n",
            ),
        ],
    );
    assert_eq!(format(&root.join("index.ts")), "const a = { b: 1 };\n");
}

/// Saves with formatting on save on and waits for the file to be saved formatted.
fn save_formatted(file: &Path, formatted: &str) {
    let mut editor = Editor::new(PLUGINS);
    editor.run_command("format-on-save-mode");
    editor.visit_file(file).unwrap();
    editor.window_buffer_mut().save().unwrap();
    refresh_until(&mut editor, "saving formatted", |_| {
        fs::read_to_string(file).unwrap() == formatted
    });
    assert!(!editor.window_buffer().is_modified());
}

#[test]
#[ignore]
fn files_are_formatted_after_each_save_when_turned_on() {
    let web = project(
        "save-prettier",
        &[
            ("package.json", "{ \"prettier\": { \"semi\": false } }\n"),
            ("index.ts", "const a={b:1};\n"),
        ],
    );
    save_formatted(&web.join("index.ts"), "const a = { b: 1 }\n");
    let rust = project(
        "save-rust",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"demo\"\nedition = \"2024\"\n",
            ),
            ("src/main.rs", "fn main(){}\n"),
        ],
    );
    save_formatted(&rust.join("src/main.rs"), "fn main() {}\n");
}

/// A region in `<style>` or `<script>` is formatted by its language's server, which also
/// explains and finds the code there (#42, stage 4).
#[test]
#[ignore]
fn servers_of_embedded_languages_format_and_explain_their_code() {
    let web = project(
        "html-embedded-format",
        &[
            ("package.json", "{}\n"),
            (
                "index.html",
                "<style>\na{color:red}\n</style>\n<script>\nconst a={b:1}\nfunction helper() {}\nhelper();\n\"a\".toUpperCase();\n</script>\n<p>hi</p>\n",
            ),
        ],
    );
    let page = web.join("index.html");
    let css = format_region(&page, Some(("a{color:red}", "CSS")));
    assert!(
        css.contains("<style>\na {\n    color: red\n}\n</style>"),
        "{css}"
    );
    let script = format_region(&page, Some(("const a={b:1}", "JavaScript")));
    assert!(
        script.contains("<script>\nconst a = { b: 1 }\n"),
        "{script}"
    );

    // Asked right after opening, before the page is parsed.
    assert_eq!(documentation(&page, "toUpperCase").kind, "method");
    let (path, line) = definition(&page, "helper();");
    assert_eq!(path, page.canonicalize().unwrap());
    assert_eq!(line, "function helper() {}");

    rename_symbol(&page, "helper();", "assist");
    let renamed = fs::read_to_string(&page).unwrap();
    assert!(
        renamed.contains("function assist() {}\nassist();"),
        "{renamed}"
    );
}

/// With `embedded-code = false` in `lsp.toml`, the page's code is its file's server's:
/// no document of it is opened, so nothing checks it while the same page with the setting
/// on has its problems, and the HTML server completes CSS.
#[test]
#[ignore]
fn embedded_code_off_leaves_the_page_to_its_server() {
    let web = project(
        "html-embedded-off",
        &[
            ("package.json", "{}\n"),
            (
                "index.html",
                "<style>\na { colo }\n</style>\n<script>\nlet x = ;\n</script>\n",
            ),
        ],
    );
    let page = web.join("index.html");
    let mut off = Editor::new(PLUGINS);
    let lsp_toml = (
        PathBuf::from("lsp.toml"),
        "embedded-code = false\n".to_owned(),
    );
    off.set_settings(Settings::from_files([lsp_toml]));
    assert!(offers_in(&mut off, &page, "colo", "color", TIMEOUT));
    // Opened later, the page with its code served shows how long checking it takes.
    let mut on = Editor::new(PLUGINS);
    on.visit_file(&page).unwrap();
    let checked = wait_until(&mut on, |problems| problems.len() >= 2);
    assert_eq!(problems(&mut off), [], "with the setting on: {checked:?}");
}

/// Whether completion after `before` offers `wanted` within `wait`.
fn offers(file: &Path, before: &str, wanted: &str, wait: Duration) -> bool {
    offers_in(&mut Editor::new(PLUGINS), file, before, wanted, wait)
}

fn offers_in(editor: &mut Editor, file: &Path, before: &str, wanted: &str, wait: Duration) -> bool {
    editor.visit_file(file).unwrap();
    let source = fs::read_to_string(file).unwrap();
    editor
        .window_buffer_mut()
        .set_point(source.find(before).unwrap() + before.len());
    let started = Instant::now();
    while started.elapsed() < wait {
        editor.run_command("completion-at-point");
        thread::sleep(Duration::from_millis(300));
        editor.refresh();
        if let Some(popup) = editor.popups().pop() {
            let labels: Vec<_> = popup
                .rows
                .iter()
                .take(5)
                .map(|row| row.label.clone())
                .collect();
            if popup.rows.iter().any(|row| row.label.starts_with(wanted)) {
                eprintln!("{before:?}: {wanted} offered, first {labels:?}");
                return true;
            }
        }
    }
    eprintln!("{before:?}: {wanted} not offered");
    false
}

/// Code in `<style>` and `<script>` goes to its languages' servers (#42, stage 3): the
/// HTML server alone completes CSS there, but no JavaScript or TypeScript, and checks none.
#[test]
#[ignore]
fn servers_of_embedded_languages_complete_and_check_their_code() {
    let web = project(
        "html-embedded",
        &[
            ("package.json", "{}\n"),
            (
                "index.html",
                "<style>\na { colo }\n</style>\n<script>\ndocument.getElementBy\n</script>\n\
                 <script lang=\"ts\">\nconst s: string = \"a\";\ns.toUpp\n</script>\n",
            ),
            (
                "broken.html",
                "<style>\na { colr: red; }\n</style>\n<script>\nlet x = ;\n</script>\n",
            ),
        ],
    );
    let page = web.join("index.html");
    assert!(offers(&page, "colo", "color", TIMEOUT));
    assert!(offers(&page, "getElementBy", "getElementById", TIMEOUT));
    assert!(offers(&page, "s.toUpp", "toUpperCase", TIMEOUT));
    let mut editor = Editor::new(PLUGINS);
    editor.visit_file(&web.join("broken.html")).unwrap();
    // Once each: the HTML server leaves the code to them.
    let first = wait_until(&mut editor, |problems| problems.len() >= 2);
    thread::sleep(Duration::from_secs(2));
    assert_eq!(
        problems(&mut editor),
        [
            (Icon::Warning, "Unknown property: 'colr'".into()),
            (Icon::Error, "Expression expected.".into())
        ],
        "first {first:?}"
    );
}
