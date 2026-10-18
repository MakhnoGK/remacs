use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use remacs_core::{
    Completions, ConsoleColor, ConsoleStyle, Editor, Face, GutterIcon, Mode, Section,
    parse_sequence,
};
use remacs_editing::EditingPlugin;
use remacs_minibuffer::MinibufferPlugin;

use super::*;
use crate::ansi::{Parser, Segment};
use crate::entry_points::{EntryPoint, entry_points};
use crate::launch::{
    Launch, detect_package_manager, launch, parse_dotenv, parse_version, typescript_command,
};
use crate::sessions::{State, of_window};
use crate::settings::{CompileSettings, TypeScriptRuntime};

const WAIT: Duration = Duration::from_secs(10);

/// An empty directory of its own for `test`.
fn directory(test: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("remacs-run-{}-{test}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    directory.canonicalize().unwrap()
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
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

fn editor(home: &Path) -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &ModesPlugin, &RunPlugin]);
    editor.set_settings(Default::default());
    keep_configurations_in(&mut editor, home);
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

/// Refreshes as the UI would until `done` holds.
fn wait_until(editor: &mut Editor, done: impl Fn(&Editor) -> bool) {
    let start = Instant::now();
    while !done(editor) {
        assert!(
            start.elapsed() < WAIT,
            "gave up waiting; console:\n{}",
            contents(editor)
        );
        thread::sleep(Duration::from_millis(10));
        editor.refresh();
    }
}

fn shell(name: &str, script: &str) -> Configuration {
    let mut configuration = Configuration::new(
        name,
        Kind::Executable {
            program: "sh".into(),
        },
    );
    configuration.args = vec!["-c".into(), script.into()];
    configuration
}

fn text(segments: &[Segment]) -> Vec<(&str, ConsoleStyle)> {
    segments
        .iter()
        .filter_map(|segment| match segment {
            Segment::Text(text, style) => Some((text.as_str(), *style)),
            Segment::Return => None,
        })
        .collect()
}

#[test]
fn escape_codes_style_the_text_and_are_left_out() {
    let mut parser = Parser::default();
    let segments =
        parser.feed(b"\x1b[1;32m   Compiling\x1b[0m api\x1b[38;5;208m!\x1b[38;2;1;2;3mx\x1b[K");
    let bold_green = ConsoleStyle {
        color: Some(ConsoleColor::Indexed(2)),
        bold: true,
        ..ConsoleStyle::default()
    };
    assert_eq!(
        text(&segments),
        [
            ("   Compiling", bold_green),
            (" api", ConsoleStyle::default()),
            (
                "!",
                ConsoleStyle {
                    color: Some(ConsoleColor::Indexed(208)),
                    ..ConsoleStyle::default()
                }
            ),
            (
                "x",
                ConsoleStyle {
                    color: Some(ConsoleColor::Rgb(0x010203)),
                    ..ConsoleStyle::default()
                }
            ),
        ]
    );
}

#[test]
fn escape_codes_and_characters_split_across_chunks_are_put_together() {
    let mut parser = Parser::default();
    let mut segments = parser.feed(b"a\x1b[3");
    segments.extend(parser.feed(b"1mb\xd0"));
    segments.extend(parser.feed(b"\xb6\x1b]8;;http://x\x07c"));
    let red = ConsoleStyle {
        color: Some(ConsoleColor::Indexed(1)),
        ..ConsoleStyle::default()
    };
    assert_eq!(
        text(&segments),
        [("a", ConsoleStyle::default()), ("b", red), ("жc", red)]
    );
}

#[test]
fn a_carriage_return_redraws_the_line_unless_a_newline_follows() {
    let mut parser = Parser::default();
    let mut segments = parser.feed(b"one\r\ntwo 50%\r");
    segments.extend(parser.feed(b"two 100%\n"));
    assert_eq!(
        segments,
        [
            Segment::Text("one\ntwo 50%".into(), ConsoleStyle::default()),
            Segment::Return,
            Segment::Text("two 100%\n".into(), ConsoleStyle::default()),
        ]
    );
}

fn settings() -> RunSettings {
    RunSettings {
        env: Default::default(),
        ..RunSettings::default()
    }
}

#[test]
fn cargo_and_npm_configurations_launch_their_tools() {
    let root = directory("launch");
    let mut api = Configuration::new(
        "api",
        Kind::Cargo(Cargo {
            package: Some("api".into()),
            example: Some("seed".into()),
            cargo_args: vec!["--release".into()],
            ..Cargo::default()
        }),
    );
    api.args = vec!["--port".into(), "3000".into()];
    let launched = launch(&api, &root, &settings()).unwrap();
    assert_eq!(
        launched.command_line(),
        "cargo run -p api --example seed --release -- --port 3000"
    );
    assert_eq!(launched.cwd, root);

    let mut tests = Configuration::new(
        "api tests",
        Kind::Cargo(Cargo {
            package: Some("api".into()),
            lib: true,
            command: "test".into(),
            filter: Some("store::tests::reads".into()),
            ..Cargo::default()
        }),
    );
    tests.args = vec!["--exact".into()];
    assert_eq!(
        launch(&tests, &root, &settings()).unwrap().command_line(),
        "cargo test -p api --lib store::tests::reads -- --exact"
    );

    write(&root.join("pnpm-lock.yaml"), "");
    write(&root.join("apps/site/package.json"), "{}");
    let mut site = Configuration::new(
        "site: dev",
        Kind::Npm {
            package_json: "apps/site/package.json".into(),
            script: "dev".into(),
            package_manager: None,
        },
    );
    site.args = vec!["--port".into(), "5173".into()];
    site.env.insert("A".into(), "from config".into());
    site.env_file = Some(".env".into());
    write(
        &root.join("apps/site/.env"),
        "export A=from file\nB=\"two words\" # note\n",
    );
    let launched = launch(&site, &root, &settings()).unwrap();
    assert_eq!(launched.command_line(), "pnpm run dev --port 5173");
    assert_eq!(launched.cwd, root.join("apps/site"));
    assert_eq!(
        launched.env,
        [
            ("A".into(), "from file".into()),
            ("B".into(), "two words".into()),
            ("A".into(), "from config".into()),
        ]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn npm_takes_script_arguments_after_a_separator() {
    let root = directory("npm");
    assert_eq!(detect_package_manager(&root, &root), PackageManager::Npm);
    let mut test = Configuration::new(
        "test",
        Kind::Npm {
            package_json: "package.json".into(),
            script: "test".into(),
            package_manager: None,
        },
    );
    test.args = vec!["--watch".into()];
    let launched = launch(&test, &root, &settings()).unwrap();
    assert_eq!(launched.command_line(), "npm run test -- --watch");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn typescript_runs_on_node_when_it_strips_types_else_on_tsx() {
    let no_tsx = || None;
    let tsx = || Some(PathBuf::from("/p/node_modules/.bin/tsx"));
    let auto = TypeScriptRuntime::Auto;
    assert_eq!(
        typescript_command(auto, Some((22, 6, 0)), no_tsx).unwrap(),
        ("node".into(), vec!["--experimental-strip-types".into()])
    );
    assert_eq!(
        typescript_command(auto, Some((24, 1, 0)), no_tsx).unwrap(),
        ("node".into(), vec![])
    );
    assert_eq!(
        typescript_command(auto, Some((20, 11, 0)), tsx).unwrap(),
        ("/p/node_modules/.bin/tsx".into(), vec![])
    );
    assert!(typescript_command(auto, None, no_tsx).is_err());
    assert_eq!(
        typescript_command(TypeScriptRuntime::Tsx, Some((24, 0, 0)), tsx)
            .unwrap()
            .0,
        "/p/node_modules/.bin/tsx"
    );
    assert_eq!(parse_version("v22.18.1\n"), Some((22, 18, 1)));
}

#[test]
fn command_lines_quote_what_a_shell_would_split() {
    let launch = Launch {
        program: "/usr/bin/env".into(),
        args: vec!["a b".into(), "it's".into(), "--x=1".into()],
        cwd: PathBuf::new(),
        env: Vec::new(),
        before: Vec::new(),
        shown: None,
    };
    assert_eq!(launch.command_line(), r"env 'a b' 'it'\''s' --x=1");
    assert_eq!(
        parse_dotenv("# c\n\nX='a=b'\n"),
        [("X".into(), "a=b".into())]
    );
}

fn names(points: &[EntryPoint]) -> Vec<(usize, &str)> {
    points
        .iter()
        .map(|point| (point.line, point.configuration.name.as_str()))
        .collect()
}

#[test]
fn rust_binaries_and_examples_have_their_main_as_entry_point() {
    let root = directory("rust");
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"api\"]\nexclude = [\"vendor/lib\"]\n",
    );
    write(
        &root.join("api/Cargo.toml"),
        "[package]\nname = \"api\"\n\n[[bin]]\nname = \"serve\"\npath = \"src/serve.rs\"\n",
    );
    let main = "use x;\n\n#[tokio::main]\nasync fn main() -> Result<()> {\n}\n";
    let entry = |path: &str| {
        let path = root.join(path);
        write(&path, main);
        entry_points(&path, "Rust", main, &root)
    };
    let points = entry("api/src/main.rs");
    assert_eq!(names(&points), [(3, "api")]);
    assert_eq!(
        points[0].configuration.kind,
        Kind::Cargo(Cargo {
            package: Some("api".into()),
            bin: Some("api".into()),
            ..Cargo::default()
        })
    );
    assert_eq!(points[0].configuration.cwd.as_deref(), Some("."));
    assert_eq!(names(&entry("api/src/bin/tool.rs")), [(3, "tool")]);
    assert_eq!(names(&entry("api/src/serve.rs")), [(3, "serve")]);
    assert_eq!(names(&entry("api/examples/seed/main.rs")), [(3, "seed")]);
    assert!(entry("api/src/lib.rs").is_empty());
    // A package the workspace leaves out is run from its own directory.
    write(
        &root.join("vendor/lib/Cargo.toml"),
        "[package]\nname = \"lib\"\n",
    );
    let vendored = entry("vendor/lib/examples/demo.rs");
    assert_eq!(vendored[0].configuration.cwd.as_deref(), Some("vendor/lib"));
    fs::remove_dir_all(root).unwrap();
}

/// A test entry point's lines, name, filter and arguments.
type TestRow<'a> = (usize, usize, &'a str, Option<&'a str>, &'a [String]);

fn test_filters(points: &[EntryPoint]) -> Vec<TestRow<'_>> {
    points
        .iter()
        .filter(|point| point.temporary)
        .map(|point| {
            let Kind::Cargo(cargo) = &point.configuration.kind else {
                panic!("not cargo: {:?}", point.configuration);
            };
            assert_eq!(cargo.command, "test");
            (
                point.line,
                point.end_line,
                point.configuration.name.as_str(),
                cargo.filter.as_deref(),
                point.configuration.args.as_slice(),
            )
        })
        .collect()
}

#[test]
fn rust_tests_and_modules_holding_them_are_temporary_entry_points() {
    let root = directory("rust-tests");
    write(&root.join("Cargo.toml"), "[package]\nname = \"api\"\n");
    write(&root.join("src/lib.rs"), "");
    let store = "\
pub fn read() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads() {
        read();
    }

    /// Writes.
    #[tokio::test(flavor = \"multi_thread\")]
    async fn writes() {}

    fn helper() {}

    mod none {
        fn helper() {}
    }
}
";
    let entry = |path: &str, text: &str| {
        let path = root.join(path);
        write(&path, text);
        entry_points(&path, "Rust", text, &root)
    };
    let points = entry("src/store/mod.rs", store);
    let exact = ["--exact".to_owned()];
    assert_eq!(
        test_filters(&points),
        [
            (3, 20, "store::tests", Some("store::tests::"), &[][..]),
            (
                7,
                9,
                "store::tests::reads",
                Some("store::tests::reads"),
                &exact[..]
            ),
            (
                13,
                13,
                "store::tests::writes",
                Some("store::tests::writes"),
                &exact[..]
            ),
        ]
    );
    let Kind::Cargo(cargo) = &points[0].configuration.kind else {
        unreachable!()
    };
    assert_eq!(cargo.selection(), ["-p", "api", "--lib"]);
    assert_eq!(points[0].configuration.cwd.as_deref(), Some("."));

    let integration = entry("tests/http.rs", "#[test]\nfn ok() {}\n");
    assert_eq!(
        test_filters(&integration),
        [(1, 1, "ok", Some("ok"), &exact[..])]
    );
    let Kind::Cargo(cargo) = &integration[0].configuration.kind else {
        unreachable!()
    };
    assert_eq!(cargo.selection(), ["-p", "api", "--test", "http"]);

    // A binary's main and its tests.
    let main = entry("src/main.rs", "fn main() {}\n\n#[test]\nfn starts() {}\n");
    assert_eq!(names(&main), [(0, "api"), (3, "starts")]);
    assert!(!main[0].temporary && main[1].temporary);
    let Kind::Cargo(cargo) = &main[1].configuration.kind else {
        unreachable!()
    };
    assert_eq!(cargo.selection(), ["-p", "api", "--bin", "api"]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_test_run_is_kept_as_the_temporary_configuration_until_saved() {
    let home = directory("temporary");
    let root = home.join("project");
    let mut store = configuration::Store::default();
    store.keep_in(home.join("run"), home.join("state/run.json"));
    let test = |name: &str| {
        Configuration::new(
            name,
            Kind::Cargo(Cargo {
                command: "test".into(),
                filter: Some(name.into()),
                ..Cargo::default()
            }),
        )
    };
    store.find_or_keep_temporary(&root, test("reads"));
    let kept = store.find_or_keep_temporary(&root, test("writes"));
    let project = store.project(&root).0;
    assert_eq!(project.temporary.as_ref(), Some(&kept));
    assert!(project.find("writes").is_some() && project.find("reads").is_none());
    assert!(!project.path().unwrap().exists(), "nothing is saved");

    // Saving it makes it permanent, under its own name.
    let saved = store.find_or_add(&root, test("writes")).unwrap();
    assert_eq!(saved.name, "writes");
    assert!(store.project(&root).0.temporary.is_none());
    assert_eq!(store.find_or_keep_temporary(&root, test("writes")), saved);

    let path = store.add_template(&root).unwrap();
    let written = fs::read_to_string(path).unwrap();
    assert!(
        written.ends_with("# command = \"test\"\n# cargo-args = [\"--workspace\"]\n"),
        "{written}"
    );
    assert_eq!(store.project(&root).0.configurations.len(), 1);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn script_files_and_package_scripts_are_entry_points() {
    let root = directory("scripts");
    let seed = root.join("scripts/seed.ts");
    assert_eq!(
        names(&entry_points(&seed, "TypeScript", "", &root)),
        [(0, "seed.ts")]
    );
    let test = root.join("src/a.test.ts");
    assert!(entry_points(&test, "TypeScript", "", &root).is_empty());
    let package = "{\n  \"name\": \"site\",\n  \"scripts\": {\n    \"dev\": \"vite\",\n    \"test\" : \"vitest\"\n  }\n}\n";
    let points = entry_points(&root.join("apps/site/package.json"), "", package, &root);
    assert_eq!(names(&points), [(3, "site: dev"), (4, "site: test")]);
    let points = entry_points(&root.join("package.json"), "", package, &root);
    assert_eq!(names(&points), [(3, "dev"), (4, "test")]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_new_configuration_is_appended_to_the_run_file_under_a_free_name() {
    let home = directory("store");
    let root = home.join("project");
    let mut store = configuration::Store::default();
    store.keep_in(home.join("run"), home.join("state/run.json"));
    let first = store.find_or_add(&root, shell("main", "true")).unwrap();
    assert_eq!(first.name, "main");
    let again = store.find_or_add(&root, shell("other", "false")).unwrap();
    assert_eq!(again.name, "main", "the same program finds the saved one");
    let other_program = Configuration::new(
        "main",
        Kind::Executable {
            program: "build/main".into(),
        },
    );
    let second = store.find_or_add(&root, other_program).unwrap();
    assert_eq!(second.name, "main (2)");
    let path = store.project(&root).0.path().unwrap().to_path_buf();
    let written = fs::read_to_string(&path).unwrap();
    let header = format!("# Run configurations of\n# {}\n", root.display());
    assert!(written.starts_with(&header), "{written}");
    let tables = written.lines().filter(|line| *line == "[[configurations]]");
    assert_eq!(tables.count(), 2);

    // Edited by hand, it is read again.
    thread::sleep(Duration::from_millis(20));
    fs::write(
        &path,
        written.replace("name = \"main (2)\"", "name = \"fails\""),
    )
    .unwrap();
    let names: Vec<String> = store
        .project(&root)
        .0
        .configurations
        .iter()
        .map(|configuration| configuration.name.clone())
        .collect();
    assert_eq!(names, ["main", "fails"]);

    store.select(&root, "fails").unwrap();
    let mut reopened = configuration::Store::default();
    reopened.keep_in(home.join("run"), home.join("state/run.json"));
    assert_eq!(reopened.selected(&root), Some("fails"));
    fs::remove_dir_all(home).unwrap();
}

fn styles(editor: &Editor) -> Vec<(String, Face)> {
    let buffer = editor.window_buffer();
    let len = buffer.text().len();
    editor
        .decorations(0..len)
        .into_iter()
        .flatten()
        .map(|span| (buffer.contents(span.range), span.face))
        .collect()
}

#[test]
fn a_run_shows_its_output_colored_and_how_it_ended() {
    let home = directory("output");
    let mut editor = editor(&home);
    let root = home.join("project");
    fs::create_dir_all(&root).unwrap();
    let script = r"printf '\033[31mfailed\033[0m at src/x.rs:2:5\n'; exit 3";
    commands::run_configuration(&mut editor, &root, shell("check", script), None).unwrap();
    assert_eq!(editor.window_buffer().name(), "*run: check*");
    wait_until(&mut editor, |editor| {
        contents(editor).contains("Process exited")
    });
    assert_eq!(
        contents(&editor),
        "sh -c 'printf '\\''\\033[31mfailed\\033[0m at src/x.rs:2:5\\n'\\''; exit 3'\n\
         failed at src/x.rs:2:5\nProcess exited with code 3\n"
    );
    let found = styles(&editor);
    let red = Face::Console(console::colored(1));
    assert!(found.contains(&("failed".into(), red)), "{found:?}");
    assert!(
        found.contains(&("src/x.rs:2:5".into(), Face::Link)),
        "{found:?}"
    );
    let id = of_window(&editor).unwrap();
    assert_eq!(
        sessions::get(&editor, id).unwrap().state,
        State::Exited(process::Exit::Code(3))
    );
    // The project now runs it with C-r.
    assert_eq!(
        editor.find_plugin_state::<Store>().unwrap().selected(&root),
        Some("check")
    );

    // A link opens its file at its line and column.
    write(&root.join("src/x.rs"), "a\nbcdefg\n");
    press(&mut editor, "M-<");
    press(&mut editor, "TAB");
    press(&mut editor, "RET");
    assert_eq!(editor.window_buffer().name(), "x.rs");
    assert_eq!(
        (
            editor.window_buffer().point_line(),
            editor.window_buffer().point_column()
        ),
        (1, 4)
    );
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_running_program_reads_input_and_stops_when_asked() {
    let home = directory("input");
    let mut editor = editor(&home);
    let root = home.join("project");
    fs::create_dir_all(&root).unwrap();
    commands::run_configuration(
        &mut editor,
        &root,
        shell("echo", "read line; echo got $line; sleep 30"),
        None,
    )
    .unwrap();
    press(&mut editor, "M->");
    press(&mut editor, "RET");
    for key in ["h", "i", "RET"] {
        press(&mut editor, key);
    }
    wait_until(&mut editor, |editor| contents(editor).contains("got hi"));
    press(&mut editor, "s-<f2>");
    wait_until(&mut editor, |editor| {
        contents(editor).ends_with("Process stopped\n")
    });
    let id = of_window(&editor).unwrap();
    assert_eq!(sessions::get(&editor, id).unwrap().state, State::Stopped);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn rerunning_a_running_program_stops_it_first_and_reuses_its_console() {
    let home = directory("rerun");
    let mut editor = editor(&home);
    let root = home.join("project");
    fs::create_dir_all(&root).unwrap();
    let looping = shell("loop", "echo started; sleep 30");
    commands::run_configuration(&mut editor, &root, looping, None).unwrap();
    wait_until(&mut editor, |editor| contents(editor).contains("started"));
    press(&mut editor, "C-r");
    wait_until(&mut editor, |editor| {
        let text = contents(editor);
        !text.contains("stopped")
            && text.contains("started")
            && of_window(editor)
                .and_then(|id| sessions::get(editor, id))
                .is_some_and(|session| {
                    session.state == State::Running && session.finished.is_none()
                })
    });
    assert_eq!(editor.buffers().len(), 2, "one console and *scratch*");
    press(&mut editor, "C-c C-c");
    wait_until(&mut editor, |editor| {
        contents(editor).ends_with("Process stopped\n")
    });
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_failed_step_before_launch_stops_the_run() {
    let home = directory("before");
    let mut editor = editor(&home);
    let root = home.join("project");
    fs::create_dir_all(&root).unwrap();
    let mut configuration = shell("main", "echo main ran");
    configuration.before_launch = vec!["echo building".into(), "exit 2".into()];
    commands::run_configuration(&mut editor, &root, configuration, None).unwrap();
    wait_until(&mut editor, |editor| {
        contents(editor).contains("Process exited")
    });
    assert_eq!(
        contents(&editor),
        "echo building\nbuilding\nexit 2\nProcess exited with code 2\n"
    );
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn the_gutter_marks_entry_points_and_its_menu_runs_them() {
    let home = directory("gutter");
    let mut editor = editor(&home);
    let root = home.join("project");
    fs::create_dir_all(root.join(".git")).unwrap();
    let script = root.join("hello.mjs");
    write(&script, "console.log('hello')\n");
    editor.visit_file(&script).unwrap();
    wait_until(&mut editor, |editor| {
        editor
            .gutter_marks(0..10)
            .is_some_and(|marks| !marks.is_empty())
    });
    let marks = editor.gutter_marks(0..10).unwrap();
    assert_eq!(
        marks
            .iter()
            .map(|mark| (mark.line, mark.icon))
            .collect::<Vec<_>>(),
        [(0, GutterIcon::Run)]
    );
    editor.run_command(window::ENTRY_POINT_MENU);
    let popup = editor.popups().pop().unwrap();
    let rows: Vec<(String, Option<&str>)> = popup
        .rows
        .iter()
        .map(|row| (row.label.clone(), row.tag))
        .collect();
    assert_eq!(
        rows,
        [
            ("Run 'hello.mjs'".into(), Some("new")),
            ("Edit configuration…".into(), None)
        ]
    );
    press(&mut editor, "C-n");
    press(&mut editor, "RET");
    let run_file = editor.window_buffer().path().unwrap().to_path_buf();
    assert!(
        run_file.starts_with(home.join("run")),
        "{}",
        run_file.display()
    );
    assert_eq!(
        editor.window_buffer().contents(
            editor
                .window_buffer()
                .text()
                .line_range(editor.window_buffer().point_line())
                .unwrap()
        ),
        "name = \"hello.mjs\""
    );
    let text = fs::read_to_string(&run_file).unwrap();
    assert!(
        text.contains("kind = \"node\"\nfile = \"hello.mjs\""),
        "{text}"
    );
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn the_chooser_lists_configurations_then_what_the_project_can_run() {
    let home = directory("chooser");
    let mut editor = editor(&home);
    let root = home.join("project");
    fs::create_dir_all(root.join(".git")).unwrap();
    let package = "{\n  \"scripts\": {\n    \"dev\": \"vite\",\n    \"test\": \"vitest\"\n  }\n}\n";
    write(&root.join("apps/site/package.json"), package);
    write(&root.join("README"), "");
    configuration_store(&mut editor)
        .find_or_add(&root, shell("check", "true"))
        .unwrap();
    editor.visit_file(&root.join("README")).unwrap();
    editor.refresh();
    press(&mut editor, "M-R");
    let listed = |editor: &Editor| -> Vec<(String, Option<&'static str>, bool)> {
        editor
            .minibuffer()
            .map(|minibuffer| {
                minibuffer
                    .candidates()
                    .iter()
                    .map(|candidate| (candidate.label.clone(), candidate.verb, candidate.dim))
                    .collect()
            })
            .unwrap_or_default()
    };
    wait_until(&mut editor, |editor| listed(editor).len() == 4);
    assert_eq!(
        listed(&editor),
        [
            ("site: dev".into(), Some("New"), true),
            ("site: test".into(), Some("New"), true),
            ("check".into(), None, false),
            ("New configuration…".into(), None, false),
        ]
    );
    // C-e saves a new one and edits it in the run file.
    press(&mut editor, "C-e");
    let visited = editor.window_buffer().path().map(Path::to_path_buf);
    assert_eq!(
        visited.as_deref(),
        configuration_store(&mut editor).project(&root).0.path()
    );
    assert_eq!(
        contents(&editor)
            .lines()
            .nth(editor.window_buffer().point_line()),
        Some("name = \"site: dev\"")
    );
    let saved: Vec<String> = configuration_store(&mut editor)
        .project(&root)
        .0
        .configurations
        .iter()
        .map(|configuration| configuration.name.clone())
        .collect();
    assert_eq!(saved, ["check", "site: dev"]);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_console_redraws_its_last_line_and_keeps_its_newest_lines() {
    let mut buffer = console::new_buffer("*run*".into(), 1, Path::new("/"), Path::new("/"));
    console::append_output(&mut buffer, b"\x1b[32mok\x1b[0m\nbuild 10%\r", 1_000);
    console::append_output(&mut buffer, b"build 100%\n", 1_000);
    let text = |buffer: &remacs_core::Buffer| buffer.contents(0..buffer.text().len());
    assert_eq!(text(&buffer), "ok\nbuild 100%\n");
    let lines: String = (0..300).map(|line| format!("line {line}\n")).collect();
    console::append_output(&mut buffer, lines.as_bytes(), 100);
    assert_eq!(buffer.text().line_count(), 100);
    assert!(text(&buffer).starts_with("line 201\n"));
    assert!(console::style_spans(&buffer, 0..buffer.text().len()).is_empty());
    assert!(!buffer.is_modified());
}

#[test]
fn run_settings_have_sane_bounds() {
    assert!(RunSettings::default().check().is_ok());
    let settings: RunSettings = toml::from_str("console-lines = 5\n").unwrap();
    assert!(settings.check().is_err());
    let settings: RunSettings = toml::from_str("[compile]\nhistory = 0\n").unwrap();
    assert!(settings.check().is_err());
}

/// An editor whose compile commands run on `sh`, whatever `$SHELL` is.
fn compile_editor(home: &Path) -> Editor {
    let mut editor = editor(home);
    sessions::sessions(&mut editor).settings.compile.shell = "/bin/sh".into();
    editor
}

fn message(editor: &Editor) -> String {
    editor
        .message()
        .map(|message| message.text.clone())
        .unwrap_or_default()
}

fn position(editor: &Editor) -> (&str, usize, usize) {
    let buffer = editor.window_buffer();
    (buffer.name(), buffer.point_line(), buffer.point_column())
}

#[test]
fn shell_configurations_run_their_command_line_on_the_configured_shell() {
    let mut configuration = Configuration::new(
        "lint",
        Kind::Shell {
            command: "cargo clippy | tee out".into(),
        },
    );
    configuration.args = vec!["a b".into()];
    let mut settings = settings();
    settings.compile.shell = "/bin/zsh".into();
    let launch = launch(&configuration, Path::new("/src/shop"), &settings).unwrap();
    assert_eq!(launch.program, "/bin/zsh");
    assert_eq!(launch.args, ["-c", "cargo clippy | tee out 'a b'"]);
    assert_eq!(launch.cwd, Path::new("/src/shop"));
    assert_eq!(launch.command_line(), "cargo clippy | tee out 'a b'");
}

#[test]
fn compile_runs_a_command_line_and_next_error_visits_its_locations_in_turn() {
    let home = directory("compile");
    let mut editor = compile_editor(&home);
    let root = home.join("project");
    fs::create_dir_all(root.join(".git")).unwrap();
    write(&root.join("src/a.rs"), "one\ntwo\n");
    write(&root.join("src/b.rs"), "three\n");
    write(
        &root.join("build.sh"),
        "echo 'src/a.rs:2:1: error'\necho 'see https://x.dev and src/b.rs:1:3'\nexit 1\n",
    );
    let script = "sh build.sh";
    compile::run(&mut editor, &root, script).unwrap();
    assert_eq!(editor.window_buffer().name(), "*compile: project*");
    wait_until(&mut editor, |editor| {
        contents(editor).contains("Process exited")
    });
    assert_eq!(
        contents(&editor),
        format!(
            "{script}\nsrc/a.rs:2:1: error\nsee https://x.dev and src/b.rs:1:3\n\
             Process exited with code 1\n"
        )
    );
    assert_eq!(
        message(&editor),
        "Compilation exited abnormally with code 1"
    );
    // The project's chosen configuration stays as it was.
    assert_eq!(
        editor.find_plugin_state::<Store>().unwrap().selected(&root),
        None
    );

    press(&mut editor, "M-<");
    press(&mut editor, "M-g M-n");
    assert_eq!(position(&editor), ("a.rs", 1, 0));
    press(&mut editor, "M-g M-n");
    assert_eq!(position(&editor), ("b.rs", 0, 2), "URLs are skipped");
    press(&mut editor, "M-g M-n");
    assert_eq!(message(&editor), "No more locations");
    press(&mut editor, "M-g M-p");
    assert_eq!(position(&editor), ("a.rs", 1, 0));

    // g in the console runs the same command again, in the same console.
    let buffers = editor.buffers().len();
    editor.switch_to_buffer("*compile: project*");
    press(&mut editor, "g");
    wait_until(&mut editor, |editor| {
        of_window(editor)
            .and_then(|id| sessions::get(editor, id))
            .is_some_and(|session| session.finished.is_some())
            && contents(editor).contains("Process exited")
    });
    assert_eq!(editor.buffers().len(), buffers);
    assert!(contents(&editor).starts_with(script));
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn compile_commands_are_remembered_per_project_newest_first() {
    let home = directory("compile-history");
    let mut editor = compile_editor(&home);
    let root = home.join("project");
    write(&root.join("Cargo.toml"), "");
    for command in ["echo one", "echo two", "echo one"] {
        compile::run(&mut editor, &root, command).unwrap();
        wait_until(&mut editor, |editor| {
            contents(editor).contains("Process exited")
        });
    }
    assert_eq!(message(&editor), "Compilation finished");
    assert_eq!(
        editor.buffers().len(),
        2,
        "one compile console and *scratch*"
    );

    let mut reopened = compile::History::default();
    reopened.keep_in(home.join("state/compile.json"));
    let commands = vec!["echo one".to_owned(), "echo two".to_owned()];
    assert_eq!(reopened.of(&root), commands);

    let labels = |completions: Completions| -> Vec<String> {
        completions
            .candidates
            .into_iter()
            .map(|candidate| candidate.value)
            .collect()
    };
    assert_eq!(
        labels(compile::candidates(&commands, Some("cargo build"), "")),
        ["echo one", "echo two", "cargo build"]
    );
    assert_eq!(
        labels(compile::candidates(&commands, None, "echo t")),
        ["echo t", "echo two"],
        "what is typed is the answer unless a command is chosen"
    );
    let settings = CompileSettings::default();
    assert_eq!(
        compile::default_command(&root, &settings).as_deref(),
        Some("cargo build")
    );
    let web = root.join("web");
    fs::create_dir_all(&web).unwrap();
    fs::write(web.join("package.json"), "{}\n").unwrap();
    assert_eq!(
        compile::default_command(&web, &settings).as_deref(),
        Some("npm run build")
    );
    let vite_plus = r#"{ "devDependencies": { "vite-plus": "1" } }"#;
    fs::write(web.join("package.json"), vite_plus).unwrap();
    assert_eq!(
        compile::default_command(&web, &settings).as_deref(),
        Some("vp check")
    );

    // From a file of the project, RET runs the last command; what is typed runs as typed.
    editor.visit_file(&root.join("Cargo.toml")).unwrap();
    press(&mut editor, "C-f");
    press(&mut editor, "s-<f9>");
    press(&mut editor, "RET");
    wait_until(&mut editor, |editor| {
        contents(editor).starts_with("echo one\n") && contents(editor).contains("Process exited")
    });
    press(&mut editor, "s-<f9>");
    for key in "echo t".chars() {
        press(&mut editor, &key.to_string().replace(' ', "SPC"));
    }
    press(&mut editor, "RET");
    wait_until(&mut editor, |editor| {
        contents(editor).starts_with("echo t\n") && contents(editor).contains("Process exited")
    });
    fs::remove_dir_all(home).unwrap();
}
