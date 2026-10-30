//! Built-in plugin: vertico-style keys for answering prompts, and `M-x`.

use remacs_core::{
    Candidate, Command, CommandResult, Completions, Count, Editor, Key, KeyCode, Keymap, MenuItem,
    Minibuffer, Plugin, Prompt, Registry, describe_sequence, filter_candidates,
};

pub struct MinibufferPlugin;

const COMMAND_CATEGORY: &str = "command";

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new("execute-extended-command", "Read a command name and run it", execute_extended_command),
    Command::new("exit-minibuffer", "Answer the prompt with the selected candidate", exit_minibuffer),
    Command::new("exit-minibuffer-with-input", "Answer the prompt with the input as typed", exit_minibuffer_with_input),
    Command::new("minibuffer-complete", "Replace the input with the selected candidate", minibuffer_complete),
    Command::new("next-candidate", "Select the next candidate", next_candidate),
    Command::new("previous-candidate", "Select the previous candidate", previous_candidate),
    Command::new("minibuffer-next-field", "Move between a message's subject and description, or the input and the list", next_field),
    Command::new("minibuffer-message-newline", "Break the description's line, or go on to it from the subject", message_newline),
    Command::new("previous-message", "Bring back the message before this one", previous_message),
    Command::new("next-message", "Bring back the message after this one", next_message),
];

const BINDINGS: &[(&str, &str)] =
    &[("M-x", "execute-extended-command"), ("s-P", "execute-extended-command")];

const MINIBUFFER_BINDINGS: &[(&str, &str)] = &[
    ("RET", "exit-minibuffer"),
    ("M-RET", "exit-minibuffer-with-input"),
    ("TAB", "minibuffer-complete"),
    ("C-n", "next-candidate"),
    ("<down>", "next-candidate"),
    ("C-p", "previous-candidate"),
    ("<up>", "previous-candidate"),
];

/// Like Emacs `git-commit-mode` and `log-edit-mode`.
const MESSAGE_BINDINGS: &[(&str, &str)] = &[
    ("RET", "minibuffer-message-newline"),
    ("TAB", "minibuffer-next-field"),
    ("C-c C-c", "exit-minibuffer-with-input"),
    ("M-p", "previous-message"),
    ("M-n", "next-message"),
    ("C-n", "next-line"),
    ("C-p", "previous-line"),
    ("<down>", "next-line"),
    ("<up>", "previous-line"),
];

impl Plugin for MinibufferPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        for &(keys, command) in MINIBUFFER_BINDINGS {
            registry.minibuffer_keymap.bind(keys, command);
        }
        for &(keys, command) in MESSAGE_BINDINGS {
            registry.message_keymap.bind(keys, command);
        }
        registry.menu_items.push(MenuItem::new(
            "View",
            "commands",
            "Execute Command…",
            "execute-extended-command",
        ));
    }
}

fn execute_extended_command(editor: &mut Editor) -> CommandResult {
    let prompt = Prompt {
        label: "M-x".into(),
        category: COMMAND_CATEGORY,
        require_match: true,
        count: Count::Position,
        hints: &[("↵", "Run"), ("⇥", "Complete"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(prompt, command_candidates, |editor, name| {
        editor.run_command(&name);
        Ok(())
    })
}

/// Recently run commands first, then the rest alphabetically.
fn command_candidates(editor: &Editor, input: &str) -> Completions {
    let history = editor.history(COMMAND_CATEGORY);
    let mut commands: Vec<&Command> = editor.commands().iter().collect();
    commands.sort_by_key(|command| {
        let recency = history.iter().position(|name| name == command.name);
        (recency.unwrap_or(usize::MAX), command.name)
    });
    let candidates = commands
        .into_iter()
        .map(|command| Candidate {
            annotation: command.doc.into(),
            keys: preferred_keys(editor.keymap(), command.name),
            ..Candidate::new(command.name)
        })
        .collect();
    filter_candidates(input, candidates, true)
}

/// The binding worth showing: Emacs keys over Command-key ones, characters over named keys.
fn preferred_keys(keymap: &Keymap, command: &str) -> Vec<String> {
    let uses_super = |keys: &[Key]| keys.iter().any(|key| key.modifiers.super_key);
    let uses_named_key =
        |keys: &[Key]| keys.iter().any(|key| !matches!(key.code, KeyCode::Char(_)));
    keymap
        .sequences_for(command)
        .min_by_key(|keys| {
            let description = describe_sequence(keys);
            (uses_super(keys), uses_named_key(keys), description.len(), description)
        })
        .map(|keys| keys.iter().map(Key::to_string).collect())
        .unwrap_or_default()
}

fn exit_minibuffer(editor: &mut Editor) -> CommandResult {
    editor.exit_minibuffer(false)
}

fn exit_minibuffer_with_input(editor: &mut Editor) -> CommandResult {
    editor.exit_minibuffer(true)
}

fn minibuffer_complete(editor: &mut Editor) -> CommandResult {
    minibuffer(editor)?.complete()
}

fn next_candidate(editor: &mut Editor) -> CommandResult {
    minibuffer(editor)?.select_next();
    Ok(())
}

fn previous_candidate(editor: &mut Editor) -> CommandResult {
    minibuffer(editor)?.select_previous();
    Ok(())
}

fn next_field(editor: &mut Editor) -> CommandResult {
    minibuffer(editor)?.next_field()
}

/// Breaks the description's line; in the subject, goes on to the description.
fn message_newline(editor: &mut Editor) -> CommandResult {
    let minibuffer = minibuffer(editor)?;
    if !minibuffer.in_description() {
        return minibuffer.next_field();
    }
    editor.buffer_mut().insert("\n");
    Ok(())
}

fn previous_message(editor: &mut Editor) -> CommandResult {
    minibuffer(editor)?.recall_message(true)
}

fn next_message(editor: &mut Editor) -> CommandResult {
    minibuffer(editor)?.recall_message(false)
}

fn minibuffer(editor: &mut Editor) -> Result<&mut Minibuffer, &'static str> {
    editor.minibuffer_mut().ok_or("Not in the minibuffer")
}

#[cfg(test)]
mod tests;
