use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use remacs_core::{BreakpointIcon, Editor, GutterIcon, Mode, Plugin, Registry, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_minibuffer::MinibufferPlugin;
use remacs_run::{Cargo, Configuration, Kind, Launch, RunPlugin, keep_configurations_in};
use remacs_tools::{ToolsPlugin, keep_tools_in};
use serde_json::json;

use super::*;
use crate::adapter::{Adapter, plan};
use crate::breakpoints::Breakpoints;
use crate::dap::{encode, read_message};
use crate::prepare::{Built, CargoBuild, built_executable};

const WAIT: Duration = Duration::from_secs(60);

/// An empty directory of its own for `test`.
fn directory(test: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("remacs-debug-{}-{test}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    directory.canonicalize().unwrap()
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn messages_are_framed_by_their_length() {
    let first = json!({ "seq": 1, "type": "event", "event": "initialized" });
    let second =
        json!({ "seq": 2, "type": "event", "event": "output", "body": { "output": "é\n" } });
    let mut bytes = encode(&first);
    bytes.extend(encode(&second));
    let mut reader = BufReader::new(bytes.as_slice());
    assert_eq!(read_message(&mut reader).unwrap(), Some(first));
    assert_eq!(read_message(&mut reader).unwrap(), Some(second));
    assert_eq!(read_message(&mut reader).unwrap(), None);
}

#[test]
fn cargo_configurations_are_built_then_debugged_with_codelldb() {
    let configuration = Configuration {
        args: vec!["--port".into(), "3000".into()],
        ..Configuration::new(
            "api",
            Kind::Cargo(Cargo {
                package: Some("api".into()),
                cargo_args: vec!["--release".into()],
                ..Cargo::default()
            }),
        )
    };
    let launch = Launch {
        program: "cargo".into(),
        args: vec![],
        cwd: "/src/shop".into(),
        env: vec![("RUST_LOG".into(), "debug".into())],
        before: vec![],
    };
    let (adapter, plan) = plan(&configuration, &launch).unwrap();
    assert_eq!(adapter, Adapter::CodeLldb);
    assert_eq!(
        plan.cargo_build,
        Some(CargoBuild {
            tests: false,
            args: vec!["-p".into(), "api".into(), "--release".into()],
        })
    );
    assert_eq!(plan.target.args, ["--port", "3000"]);

    let line = r#"{"reason":"compiler-artifact","manifest_path":"/src/shop/api/Cargo.toml","target":{"name":"api","kind":["bin"]},"profile":{"test":false},"executable":"/src/shop/target/debug/api"}"#;
    assert_eq!(
        built_executable(line),
        Some(Built {
            executable: "/src/shop/target/debug/api".into(),
            label: "api (bin)".into(),
            package_dir: "/src/shop/api".into(),
            test: false,
        })
    );
    assert_eq!(built_executable(r#"{"reason":"build-finished"}"#), None);

    let mut target = plan.target;
    target.program = "/src/shop/target/debug/api".into();
    let arguments = adapter.launch_arguments("api", &target, true);
    assert_eq!(arguments["program"], "/src/shop/target/debug/api");
    assert_eq!(arguments["env"]["RUST_LOG"], "debug");
    assert_eq!(arguments["sourceLanguages"], json!(["rust"]));
}

#[test]
fn cargo_tests_are_built_without_running_and_debugged_with_their_filter() {
    let configuration = Configuration {
        args: vec!["--exact".into()],
        ..Configuration::new(
            "reads",
            Kind::Cargo(Cargo {
                package: Some("api".into()),
                lib: true,
                command: "test".into(),
                filter: Some("store::tests::reads".into()),
                ..Cargo::default()
            }),
        )
    };
    let launch = Launch {
        program: "cargo".into(),
        args: vec![],
        cwd: "/src/shop".into(),
        env: vec![],
        before: vec![],
    };
    let (_, tests) = plan(&configuration, &launch).unwrap();
    assert_eq!(
        tests.cargo_build,
        Some(CargoBuild {
            tests: true,
            args: vec!["-p".into(), "api".into(), "--lib".into()],
        })
    );
    assert_eq!(tests.target.args, ["store::tests::reads", "--exact"]);

    let line = r#"{"reason":"compiler-artifact","manifest_path":"/src/shop/api/Cargo.toml","target":{"name":"store","kind":["test"]},"profile":{"test":true},"executable":"/src/shop/target/debug/deps/store-1a2b"}"#;
    let built = built_executable(line).unwrap();
    assert!(built.test);
    assert_eq!(built.label, "store (test)");

    let bench = Configuration::new(
        "bench",
        Kind::Cargo(Cargo {
            command: "bench".into(),
            ..Cargo::default()
        }),
    );
    assert!(plan(&bench, &launch).is_err());
}

#[test]
fn breakpoints_toggle_and_are_sent_with_their_conditions() {
    let mut breakpoints = Breakpoints::default();
    let file = Path::new("/src/main.rs");
    assert!(breakpoints.toggle(file, 9));
    assert!(breakpoints.toggle(file, 4));
    breakpoints.files.get_mut(file).unwrap()[1].condition = Some("x > 2".into());
    let request = breakpoints.request(file);
    assert_eq!(
        request["breakpoints"],
        json!([{ "line": 5 }, { "line": 10, "condition": "x > 2" }])
    );
    breakpoints.verify(
        file,
        &[
            json!({ "verified": true }),
            json!({ "verified": false, "message": "no code" }),
        ],
    );
    assert_eq!(breakpoints.of(file)[1].message.as_deref(), Some("no code"));
    assert!(!breakpoints.toggle(file, 4));
    assert!(!breakpoints.toggle(file, 9));
    assert!(breakpoints.files.is_empty());
}

#[test]
fn breakpoints_move_with_their_lines_are_kept_and_marked_in_the_gutter() {
    let home = directory("follow");
    let file = home.join("main.rs");
    write(&file, "fn main() {\n    let a = 1;\n    let b = 2;\n}\n");
    let mut editor = debugging_editor(&home, &home);
    keep_breakpoints_in(&mut editor, &home);
    editor.visit_file(&file).unwrap();
    editor.set_point_at(2, 0);
    press(&mut editor, "s-<f8>");
    let marks = |editor: &Editor| {
        editor
            .gutter_marks(0..10)
            .unwrap()
            .into_iter()
            .map(|mark| (mark.line, mark.icon))
            .collect::<Vec<_>>()
    };
    let plain = GutterIcon::Breakpoint(BreakpointIcon::Plain);
    assert_eq!(marks(&editor), [(2, plain)]);

    // A line opened above pushes it down; a newline typed at its start too.
    editor.set_point_at(1, 0);
    press(&mut editor, "s-RET");
    assert_eq!(marks(&editor), [(3, plain)]);
    editor.set_point_at(3, 0);
    press(&mut editor, "RET");
    assert_eq!(marks(&editor), [(4, plain)]);
    // Lines killed above pull it up.
    editor.set_point_at(1, 0);
    press(&mut editor, "C-k C-k");
    assert_eq!(marks(&editor), [(3, plain)]);

    // Kept, at its new line, for the next session.
    let mut restarted = debugging_editor(&home, &home);
    keep_breakpoints_in(&mut restarted, &home);
    restarted.visit_file(&file).unwrap();
    assert_eq!(marks(&restarted), [(3, plain)]);
    // A click beside a line without a mark adds one.
    assert_eq!(restarted.gutter_click(), Some("debug-toggle-breakpoint"));
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_breakpoint_gets_a_condition_and_the_list_disables_and_deletes_breakpoints() {
    let home = directory("edit");
    let file = home.join("main.rs");
    write(&file, "fn main() {\n    let a = 1;\n    let b = 2;\n}\n");
    let mut editor = debugging_editor(&home, &home);
    keep_breakpoints_in(&mut editor, &home);
    editor.visit_file(&file).unwrap();
    for line in [1, 2] {
        editor.set_point_at(line, 0);
        press(&mut editor, "s-<f8>");
    }
    press(&mut editor, "s-S-<f8>");
    assert_eq!(editor.minibuffer().unwrap().prompt().label, "main.rs:3");
    for key in ["a", "SPC", ">", "SPC", "0", "RET"] {
        press(&mut editor, key);
    }
    let breakpoints = &debugger(&mut editor).breakpoints;
    assert_eq!(breakpoints.of(&file)[1].condition.as_deref(), Some("a > 0"));
    let icon = |editor: &Editor, line: usize| {
        editor
            .gutter_marks(line..line + 1)
            .unwrap()
            .first()
            .map(|mark| mark.icon)
    };
    assert_eq!(
        icon(&editor, 2),
        Some(GutterIcon::Breakpoint(BreakpointIcon::Conditional))
    );

    // Without a breakpoint on the line, the key lists them all.
    editor.set_point_at(0, 0);
    press(&mut editor, "s-S-<f8>");
    let rows = |editor: &Editor| {
        editor
            .minibuffer()
            .unwrap()
            .candidates()
            .iter()
            .map(|row| {
                (
                    row.label.clone(),
                    row.annotation.clone(),
                    row.detail.clone().unwrap_or_default().into_owned(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        rows(&editor),
        [
            (
                "main.rs:2".to_owned(),
                "let a = 1;".to_owned(),
                String::new()
            ),
            (
                "main.rs:3".to_owned(),
                "let b = 2;".to_owned(),
                "if a > 0".to_owned()
            ),
        ]
    );
    press(&mut editor, "SPC");
    assert_eq!(rows(&editor)[0].2, "disabled");
    assert_eq!(
        icon(&editor, 1),
        Some(GutterIcon::Breakpoint(BreakpointIcon::Disabled))
    );
    press(&mut editor, "d");
    assert_eq!(rows(&editor).len(), 1);
    press(&mut editor, "RET");
    assert!(editor.minibuffer().is_none());
    assert_eq!(editor.window_buffer().point_line(), 2);
    fs::remove_dir_all(home).unwrap();
}

/// The major modes entry points are found in.
struct ModesPlugin;

impl Plugin for ModesPlugin {
    fn register(&self, registry: &mut Registry) {
        for (name, extensions) in [("JavaScript", &["js", "mjs"][..]), ("Rust", &["rs"][..])] {
            registry.modes.add(Mode {
                name,
                extensions,
                file_names: &[],
                pairs: &[],
                comment: None,
                markup_comment: None,
            });
        }
    }
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn contents(editor: &Editor, name: &str) -> String {
    let buffer = editor
        .buffers()
        .iter()
        .find(|buffer| buffer.name() == name)
        .unwrap();
    buffer.contents(0..buffer.text().len())
}

/// Refreshes as the UI would until `done` holds.
fn wait_until(editor: &mut Editor, what: &str, done: impl Fn(&Editor) -> bool) {
    let start = Instant::now();
    while !done(editor) {
        assert!(start.elapsed() < WAIT, "gave up waiting for {what}");
        thread::sleep(Duration::from_millis(20));
        editor.refresh();
    }
}

fn status(editor: &Editor) -> String {
    editor
        .status_segments()
        .iter()
        .flat_map(|segment| segment.items.iter().map(|item| item.text.clone()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The rows of the pane's `column`, as `name = value`, or the name alone.
fn pane_rows(editor: &Editor, column: usize) -> Vec<String> {
    let Some(pane) = editor.pane() else {
        return Vec::new();
    };
    pane.columns[column]
        .rows
        .iter()
        .map(|row| match &row.value {
            Some(value) => format!("{} = {value}", row.name),
            None => row.name.clone(),
        })
        .collect()
}

const PROGRAM: &str = r#"struct Item {
    price: u64,
    qty: u64,
}

fn total(items: &[Item]) -> u64 {
    let subtotal: u64 = items.iter().map(|i| i.price * i.qty).sum();
    let discount = 15;
    subtotal - subtotal * discount / 100
}

fn main() {
    let items = vec![Item { price: 990, qty: 2 }, Item { price: 1300, qty: 1 }];
    let t = total(&items);
    println!("total {t}");
}
"#;

/// Needs codelldb installed by remacs; skipped when it is not.
#[test]
fn a_cargo_program_stops_at_a_breakpoint_steps_and_runs_to_its_end() {
    let Some(real_home) = remacs_config::home() else {
        return;
    };
    if !real_home.join("cache/tools/codelldb").is_dir() {
        eprintln!("skipped: codelldb is not installed");
        return;
    }
    let home = directory("cargo");
    let root = home.join("demo");
    fs::create_dir_all(root.join(".git")).unwrap();
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
    );
    let main = root.join("src/main.rs");
    write(&main, PROGRAM);

    let mut editor = Editor::new(&[
        &EditingPlugin,
        &MinibufferPlugin,
        &ModesPlugin,
        // Before the run plugin, which then sees in the same pass what the debugger did.
        &DebugPlugin,
        &RunPlugin,
        &ToolsPlugin,
    ]);
    editor.set_settings(Default::default());
    keep_configurations_in(&mut editor, &home);
    keep_tools_in(&mut editor, &real_home);

    editor.visit_file(&main).unwrap();
    editor.set_point_at(8, 0);
    press(&mut editor, "s-<f8>");
    editor.set_point_at(11, 0);
    wait_until(&mut editor, "the entry point", |editor| {
        editor
            .gutter_marks(11..12)
            .is_some_and(|marks| !marks.is_empty())
    });
    press(&mut editor, "C-D");
    assert_eq!(editor.window_buffer().name(), "*run: demo*");

    wait_until(&mut editor, "the breakpoint", |editor| {
        editor.window_buffer().name() == "main.rs" && editor.window_buffer().point_line() == 8
    });
    assert!(status(&editor).contains("paused"), "{}", status(&editor));
    let address = current(&mut editor).unwrap();
    let session = debugger(&mut editor)
        .sessions
        .iter()
        .find(|session| session.id == address.session)
        .unwrap();
    let frames = &session.link(address.link).unwrap().frames;
    assert!(frames[0].name.contains("total"), "{frames:?}");

    // The pane shows the frames and variables; the lines above, the values they declare.
    wait_until(&mut editor, "the variables", |editor| {
        pane_rows(editor, 1)
            .iter()
            .any(|row| row.starts_with("discount = 15"))
    });
    assert!(
        pane_rows(&editor, 0)
            .iter()
            .any(|row| row.contains("total"))
    );
    assert!(pane_rows(&editor, 1).contains(&"subtotal = 3280".to_owned()));
    let extras = editor.line_extras(0..20);
    let note = |line: usize| {
        extras
            .iter()
            .find(|extra| extra.line == line)
            .and_then(|extra| extra.note.clone())
    };
    assert_eq!(note(6).as_deref(), Some("subtotal = 3280"));
    assert_eq!(note(7).as_deref(), Some("discount = 15"));
    assert!(
        extras
            .iter()
            .any(|extra| extra.line == 8 && extra.band.is_some())
    );
    // A watch, added from its column.
    press(&mut editor, "C-c w");
    press(&mut editor, "RET");
    for character in "subtotal * 2".chars() {
        press(&mut editor, &character.to_string().replace(' ', "SPC"));
    }
    press(&mut editor, "RET");
    wait_until(&mut editor, "the watch", |editor| {
        pane_rows(editor, 2).first().map(String::as_str) == Some("subtotal * 2 = 6560")
    });
    // Keys go to the pane until it gives them back.
    press(&mut editor, "x");
    assert!(!contents(&editor, "main.rs").contains('x'));
    press(&mut editor, "C-g");
    // An expression evaluated in the paused frame goes to the console.
    press(&mut editor, "M-<f8>");
    for character in "subtotal + 1".chars() {
        press(&mut editor, &character.to_string().replace(' ', "SPC"));
    }
    press(&mut editor, "RET");
    wait_until(&mut editor, "the evaluation", |editor| {
        contents(editor, "*run: demo*").contains("› subtotal + 1\n3281\n")
    });

    press(&mut editor, "S-<f8>");
    wait_until(&mut editor, "stepping out", |editor| {
        editor.window_buffer().point_line() == 13 && status(editor).contains("paused")
    });
    press(&mut editor, "<f9>");
    wait_until(&mut editor, "the end", |editor| {
        contents(editor, "*run: demo*").contains("Process exited with code 0")
    });
    let console = contents(&editor, "*run: demo*");
    assert!(
        console.contains("cargo build -p demo --bin demo"),
        "{console}"
    );
    assert!(console.contains("total 2788\n"), "{console}");
    assert!(debugger(&mut editor).sessions.is_empty());
    fs::remove_dir_all(home).unwrap();
}

fn debugging_editor(home: &Path, real_home: &Path) -> Editor {
    let mut editor = Editor::new(&[
        &EditingPlugin,
        &MinibufferPlugin,
        &ModesPlugin,
        &DebugPlugin,
        &RunPlugin,
        &ToolsPlugin,
    ]);
    editor.set_settings(Default::default());
    keep_configurations_in(&mut editor, home);
    keep_tools_in(&mut editor, real_home);
    editor
}

/// Needs js-debug installed by remacs, and Node; skipped when they are not.
#[test]
fn a_node_script_stops_at_a_breakpoint_in_the_session_js_debug_starts() {
    let Some(real_home) = remacs_config::home() else {
        return;
    };
    if !real_home.join("cache/tools/js-debug").is_dir() {
        eprintln!("skipped: js-debug is not installed");
        return;
    }
    let home = directory("node");
    let root = home.join("site");
    fs::create_dir_all(root.join(".git")).unwrap();
    let script = root.join("seed.mjs");
    write(
        &script,
        "const products = [990, 1300];\nlet sum = 0;\nfor (const price of products) {\n  sum += price;\n}\nconsole.log(`sum ${sum}`);\n",
    );
    let mut editor = debugging_editor(&home, &real_home);
    editor.visit_file(&script).unwrap();
    editor.set_point_at(3, 0);
    press(&mut editor, "s-<f8>");
    wait_until(&mut editor, "the entry point", |editor| {
        editor
            .gutter_marks(0..1)
            .is_some_and(|marks| !marks.is_empty())
    });
    press(&mut editor, "C-D");

    wait_until(&mut editor, "the breakpoint", |editor| {
        editor.window_buffer().name() == "seed.mjs" && editor.window_buffer().point_line() == 3
    });
    assert!(status(&editor).contains("paused"), "{}", status(&editor));
    press(&mut editor, "<f9>");
    wait_until(&mut editor, "the second pass", |editor| {
        status(editor).contains("paused")
    });
    press(&mut editor, "s-<f8>");
    press(&mut editor, "<f9>");
    wait_until(&mut editor, "the end", |editor| {
        contents(editor, "*run: seed.mjs*").contains("Process exited")
    });
    let console = contents(&editor, "*run: seed.mjs*");
    assert!(console.contains("sum 2290"), "{console}");
    fs::remove_dir_all(home).unwrap();
}
