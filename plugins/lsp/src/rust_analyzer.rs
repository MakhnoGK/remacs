//! rust-analyzer's own: expanding the macro call at point, and checking with clippy.

use std::path::Path;

use remacs_core::{Buffer, CommandResult, Editor};
use remacs_text::Text;
use remacs_tools::Program;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::catalog::ServerSpec;
use crate::client::Question;
use crate::{document, version};

pub(crate) const EXPAND_MACRO: &str = "expand-macro";
const NAME: &str = "rust-analyzer";
const CLIPPY: &str = "clippy";

/// What `rust-analyzer/expandMacro` answers.
#[derive(Debug, Deserialize)]
pub(crate) struct Expansion {
    name: String,
    expansion: String,
}

/// `Editor::command_count` once the `expand-macro` waiting for its answer has run.
#[derive(Default)]
struct Awaited(Option<u64>);

/// Asks the server of the code at point what the macro call there expands to.
pub(crate) fn expand_macro(editor: &mut Editor) -> CommandResult {
    document::ask_at_point(editor, Question::ExpandMacro).map_err(|server| match server {
        Some(server) => format!("{server} cannot expand macros"),
        None => "No language server expands macros here".to_owned(),
    })?;
    editor.plugin_state::<Awaited>().0 = Some(editor.command_count() + 1);
    Ok(())
}

/// Shows `expansion` read-only in `mode`, in a buffer named for its macro; an answer that
/// comes after another command is dropped, as the user went on to something else.
pub(crate) fn show(editor: &mut Editor, mode: &'static str, expansion: Option<Expansion>) {
    if editor.plugin_state::<Awaited>().0.take() != Some(editor.command_count()) {
        return;
    }
    let Some(Expansion { name, expansion }) = expansion else {
        editor.set_error("No macro call at point");
        return;
    };
    // rust-analyzer names it as called, `vec!`.
    let name = format!("*{name} expanded*");
    editor.kill_buffer(&name);
    editor.add_buffer(Buffer::new(name, Text::from(expansion.as_str()), mode).read_only());
}

/// Has rust-analyzer check with `cargo clippy`, unless its initialization options say
/// what it checks with.
pub(crate) fn check_with_clippy(specs: &mut [(String, ServerSpec, bool)]) {
    for (_, spec, _) in specs.iter_mut().filter(|(name, ..)| name == NAME) {
        let options = spec.initialization_options.get_or_insert_with(|| json!({}));
        let check =
            options.as_object_mut().map(|options| options.entry("check").or_insert(json!({})));
        if let Some(check) = check.and_then(Value::as_object_mut) {
            check.entry("command").or_insert(CLIPPY.into());
        }
    }
}

/// Leaves `initialize` checking with `cargo check`, rust-analyzer's own, where it says
/// `cargo clippy` and that does not run in `root`, as with a toolchain without clippy.
/// Asked on the thread that writes to the server, which waits for it anyway.
pub(crate) fn without_missing_clippy(
    initialize: &mut Value,
    root: &Path,
    env: &[(String, String)],
) {
    let cargo = Program { path: "cargo".into(), env: env.to_vec() };
    let runs = || version::output(&cargo, &[CLIPPY, "--version"], root, version::TIMEOUT).is_some();
    unless_clippy_runs(initialize, runs);
}

fn unless_clippy_runs(initialize: &mut Value, runs: impl FnOnce() -> bool) {
    let check = initialize.pointer_mut("/initializationOptions/check");
    let Some(check) = check.and_then(Value::as_object_mut) else {
        return;
    };
    if check.get("command").is_some_and(|command| command == CLIPPY) && !runs() {
        check.remove("command");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(options: Value) -> ServerSpec {
        let mut spec = ServerSpec::default();
        spec.initialization_options = Some(options);
        spec
    }

    #[test]
    fn an_expansion_replaces_the_last_of_its_macro_read_only_in_the_files_mode() {
        let mut editor = Editor::new(&[]);
        let answer = |expansion: &str| {
            serde_json::from_value(json!({ "name": "vec!", "expansion": expansion })).unwrap()
        };
        let awaited = |editor: &mut Editor, expansion| {
            editor.plugin_state::<Awaited>().0 = Some(editor.command_count());
            show(editor, "Rust", expansion);
        };
        awaited(&mut editor, answer("<[_]>::into_vec(old)"));
        awaited(&mut editor, answer("<[_]>::into_vec(new)"));
        // Asked for by no command since.
        show(&mut editor, "Rust", answer("late"));
        let buffers: Vec<&str> = editor.buffers().iter().map(Buffer::name).collect();
        assert_eq!(buffers.iter().filter(|name| **name == "*vec! expanded*").count(), 1);
        let buffer = editor.window_buffer();
        let shown = (
            buffer.name(),
            buffer.mode(),
            buffer.is_read_only(),
            buffer.contents(0..buffer.text().len()),
        );
        assert_eq!(shown, ("*vec! expanded*", "Rust", true, "<[_]>::into_vec(new)".to_owned()));

        awaited(&mut editor, serde_json::from_value(Value::Null).unwrap());
        assert_eq!(editor.echo().as_deref(), Some("No macro call at point"));
    }

    #[test]
    fn rust_analyzer_checks_with_clippy_unless_told_otherwise() {
        let completion = json!({ "completion": { "callable": { "snippets": "add_parentheses" } } });
        let mut specs = vec![
            (NAME.to_owned(), spec(completion), false),
            ("clangd".to_owned(), ServerSpec::default(), false),
        ];
        check_with_clippy(&mut specs);
        assert_eq!(
            specs[0].1.initialization_options,
            Some(json!({
                "completion": { "callable": { "snippets": "add_parentheses" } },
                "check": { "command": "clippy" },
            }))
        );
        assert_eq!(specs[1].1, ServerSpec::default());

        let mut own =
            vec![(NAME.to_owned(), spec(json!({ "check": { "command": "check" } })), true)];
        check_with_clippy(&mut own);
        assert_eq!(
            own[0].1.initialization_options,
            Some(json!({ "check": { "command": "check" } }))
        );
    }

    #[test]
    fn clippy_is_left_out_where_it_does_not_run() {
        let initialize = |command: &str| json!({ "initializationOptions": { "check": { "command": command, "extraArgs": [] } } });
        let checked = |command: &str, runs: bool| {
            let mut sent = initialize(command);
            unless_clippy_runs(&mut sent, || runs);
            sent
        };
        assert_eq!(checked(CLIPPY, true), initialize(CLIPPY));
        assert_eq!(
            checked(CLIPPY, false),
            json!({ "initializationOptions": { "check": { "extraArgs": [] } } })
        );
        assert_eq!(checked("check", false), initialize("check"));
        let mut bare = json!({ "initializationOptions": null });
        unless_clippy_runs(&mut bare, || panic!("asked without clippy"));
    }
}
