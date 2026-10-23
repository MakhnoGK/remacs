//! What a key or a command does, told in the echo area, like Emacs `describe-key` and
//! `describe-command`.

use remacs_core::{
    Candidate, CommandResult, Completions, Count, Editor, Prompt, describe_sequence,
    filter_candidates,
};

pub const DESCRIBE_KEY: &str = "describe-key";
pub const DESCRIBE_COMMAND: &str = "describe-command";

pub fn describe_key(editor: &mut Editor) -> CommandResult {
    editor.set_message("Describe key:");
    editor.read_key_sequence(|editor, keys, command| {
        let keys = describe_sequence(keys);
        let text = match command.and_then(|name| editor.commands().get(name)) {
            Some(command) => format!("{keys} runs {}\n{}", command.name, command.doc),
            None => format!("{keys} is undefined"),
        };
        editor.set_message(text);
    });
    Ok(())
}

pub fn describe_command(editor: &mut Editor) -> CommandResult {
    let prompt = Prompt {
        label: "Describe command".into(),
        placeholder: "Command…",
        require_match: true,
        count: Count::Position,
        hints: &[("↵", "Describe"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(prompt, command_candidates, |editor, name| {
        let text = describe(editor, &name).ok_or_else(|| format!("No command {name}"))?;
        editor.set_message(text);
        Ok(())
    })
}

fn command_candidates(editor: &Editor, input: &str) -> Completions {
    let mut commands: Vec<_> = editor.commands().iter().collect();
    commands.sort_by_key(|command| command.name);
    let candidates = commands
        .into_iter()
        .map(|command| Candidate {
            annotation: command.doc.into(),
            ..Candidate::new(command.name)
        })
        .collect();
    filter_candidates(input, candidates, true)
}

/// The command's keys on the first line, what it does under them.
fn describe(editor: &Editor, name: &str) -> Option<String> {
    let command = editor.commands().get(name)?;
    let mut keys: Vec<String> = editor
        .keymap()
        .sequences_for(name)
        .map(describe_sequence)
        .collect();
    keys.sort();
    let bound = match keys.as_slice() {
        [] => "is not bound to any key".to_owned(),
        keys => format!("is bound to {}", keys.join(", ")),
    };
    Some(format!("{name} {bound}\n{}", command.doc))
}
