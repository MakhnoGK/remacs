//! Each focused stack's fixture project, in `fixtures/`, run and debugged from its entry
//! points with nothing typed but keys (ADR 0091, stage 5).

use super::*;

/// A copy of `fixtures/<stack>` as a project of its own, in a home of its own.
fn fixture(stack: &str) -> (PathBuf, PathBuf) {
    fn copy(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap().map(Result::unwrap) {
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let home = directory(&format!("stack-{stack}"));
    let root = home.join(stack);
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(stack), &root);
    fs::create_dir_all(root.join(".git")).unwrap();
    (home, root)
}

/// Visits `file` with point on `line`, once its entry point there is found.
fn at_entry_point(editor: &mut Editor, file: &Path, line: usize) {
    editor.visit_file(file).unwrap();
    editor.set_point_at(line, 0);
    wait_until(editor, "the entry point", |editor| {
        editor.gutter_marks(line..line + 1).is_some_and(|marks| !marks.is_empty())
    });
}

fn breakpoint(editor: &mut Editor, file: &Path, line: usize) {
    editor.visit_file(file).unwrap();
    editor.set_point_at(line, 0);
    press(editor, "s-<f8>");
}

/// What every run printed so far.
fn consoles(editor: &Editor) -> String {
    let runs = editor.buffers().iter().filter(|buffer| buffer.name().starts_with("*run: "));
    runs.map(|buffer| buffer.contents(0..buffer.text().len())).collect()
}

fn wait_for_output(editor: &mut Editor, output: &str) {
    wait_until(editor, output, |editor| consoles(editor).contains(output));
}

fn paused_at(editor: &mut Editor, file: &str, line: usize) {
    wait_until(editor, "the breakpoint", |editor| {
        editor.window_buffer().name() == file
            && editor.window_buffer().point_line() == line
            && status(editor).contains("paused")
    });
}

#[test]
fn rust_runs_its_main_and_debugs_its_test() {
    let Some(real_home) = with_adapter("codelldb") else {
        return;
    };
    let (home, root) = fixture("rust");
    let main = root.join("src/main.rs");
    let mut editor = debugging_editor(&home, &real_home);

    at_entry_point(&mut editor, &main, 5);
    press(&mut editor, "C-S-<f10>");
    wait_for_output(&mut editor, "total 1947\n");

    breakpoint(&mut editor, &main, 1);
    at_entry_point(&mut editor, &main, 12);
    press(&mut editor, "C-S-<f9>");
    paused_at(&mut editor, "main.rs", 1);
    finished(&mut editor);
    // libtest colors `ok` when a debugger runs it, ending the color with `ESC ( B`.
    wait_for_output(&mut editor, "test tests::total_takes_the_discount_off ... ok\n");
    wait_for_output(&mut editor, "test result: ok. 1 passed");
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn node_runs_and_debugs_its_npm_script_and_typescript() {
    let Some(real_home) = with_adapter("js-debug") else {
        return;
    };
    let (home, root) = fixture("node");
    let (package, main) = (root.join("package.json"), root.join("src/main.ts"));
    let mut editor = debugging_editor(&home, &real_home);

    at_entry_point(&mut editor, &package, 5);
    press(&mut editor, "C-S-<f10>");
    wait_for_output(&mut editor, "total 1946.5\n");

    breakpoint(&mut editor, &main, 1);
    at_entry_point(&mut editor, &package, 5);
    press(&mut editor, "C-S-<f9>");
    paused_at(&mut editor, "main.ts", 1);
    finished(&mut editor);

    at_entry_point(&mut editor, &main, 0);
    press(&mut editor, "C-S-<f9>");
    paused_at(&mut editor, "main.ts", 1);
    finished(&mut editor);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn make_runs_its_target_and_says_it_cannot_be_debugged() {
    let (home, root) = fixture("make");
    let makefile = root.join("Makefile");
    let mut editor = debugging_editor(&home, &home);

    at_entry_point(&mut editor, &makefile, 1);
    press(&mut editor, "C-S-<f10>");
    wait_for_output(&mut editor, "built\n");

    at_entry_point(&mut editor, &makefile, 1);
    press(&mut editor, "C-S-<f9>");
    let message = editor.message().map(|message| message.text.clone());
    assert_eq!(message.as_deref(), Some("Make targets cannot be debugged"));
    fs::remove_dir_all(home).unwrap();
}
