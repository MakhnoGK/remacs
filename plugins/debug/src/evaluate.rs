//! Evaluating an expression in the paused frame, like IntelliJ's Evaluate Expression: the
//! result goes to the run's console, after the expression.

use remacs_core::{Command, CommandResult, Completions, Editor, Prompt};
use serde_json::json;

use crate::pane::shown;
use crate::session::{debugger, request};

pub const EVALUATE: &str = "debug-evaluate";
const CATEGORY: &str = "debug-expression";

pub const COMMANDS: &[Command] = &[Command::new(
    EVALUATE,
    "Evaluate an expression in the paused frame, printing the result to the console",
    evaluate,
)];

fn evaluate(editor: &mut Editor) -> CommandResult {
    let (session, address) = shown(editor).ok_or("Nothing is being debugged")?;
    let frame = session
        .link(address.link)
        .filter(|link| link.stopped.is_some())
        .and_then(|link| link.frames.get(link.frame))
        .map(|frame| frame.id);
    let prompt = Prompt {
        label: "Evaluate".into(),
        category: CATEGORY,
        placeholder: if frame.is_some() {
            "An expression, in the paused frame"
        } else {
            "An expression; the program runs, so only globals are known"
        },
        hints: &[("↵", "Evaluate"), ("M-p M-n", "History"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(
        prompt,
        |_, _| Completions::default(),
        move |editor, expression| {
            let arguments =
                json!({ "expression": expression, "frameId": frame, "context": "repl" });
            request(
                editor,
                address,
                "evaluate",
                arguments,
                move |editor, address, result| {
                    let shown = match &result {
                        Ok(body) => body["result"].as_str().unwrap_or_default().to_owned(),
                        Err(error) => error.clone(),
                    };
                    let Some(session) = debugger(editor)
                        .sessions
                        .iter()
                        .find(|session| session.id == address.session)
                    else {
                        return;
                    };
                    session.note(&format!("› {expression}"));
                    let color = if result.is_ok() { "" } else { "\x1b[31m" };
                    session.print(&format!("{color}{}\x1b[0m\n", shown.trim_end()));
                    let first = shown.lines().next().unwrap_or_default().to_owned();
                    editor.set_message(first);
                },
            );
            Ok(())
        },
    )
}
