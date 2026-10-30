//! Committing what is staged, the message written in a prompt, like Emacs `magit-commit`
//! with `git-commit-mode`.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use remacs_core::{
    CommandResult, Completions, Editor, LineCounts, ListItem, Message, Prompt, PromptOption, quote,
};

use crate::command::{git, output, output_with_input};
use crate::commands::{run, shown_status};
use crate::history_view::short_author;
use crate::monitor::Job;
use crate::status::{Head, Snapshot};
use crate::status_view::{Section, directory_label, section_files};

/// Characters a subject should keep within, as Git's own tools suggest.
const SUBJECT_LIMIT: usize = 50;
/// Messages of the last commits `M-p` brings back.
const EARLIER_MESSAGES: &str = "20";
const HINTS: &[(&str, &str)] =
    &[("C-c C-c", "Commit"), ("⇥", "Next field"), ("M-p", "Previous message"), ("C-g", "Cancel")];

/// Reads a message and commits what is staged with it; amending, an empty message keeps
/// the last commit's.
pub fn commit(editor: &mut Editor) -> CommandResult {
    let (root, snapshot) = shown_status(editor)?;
    let staged = staged_items(&snapshot);
    if staged.is_empty() && snapshot.is_unborn() {
        return Err("Nothing staged to commit".into());
    }
    let [sign_off, amend, skip_hooks] = [(); 3].map(|_| Rc::new(Cell::new(false)));
    let earlier = if snapshot.is_unborn() { Vec::new() } else { earlier_messages(&root) };
    let nothing_staged = staged.is_empty();
    let amending = amend.clone();
    let check = move |message: &str| match () {
        _ if amending.get() => Ok(()),
        _ if nothing_staged => Err("Nothing staged to commit".to_owned()),
        _ if message.trim().is_empty() => Err("The commit message is empty".to_owned()),
        _ => Ok(()),
    };
    let prompt = Prompt {
        label: "Commit".into(),
        placeholder: "What the commit does",
        wide: true,
        hints: HINTS,
        options: vec![
            PromptOption::chip('s', "Sign-off", &sign_off),
            PromptOption::chip('a', "Amend last commit", &amend),
            PromptOption::chip('h', "Skip hooks", &skip_hooks),
        ],
        subject: format!("{} → {}", author(&root), branch(&snapshot.head)),
        message: Some(Message {
            subject_limit: SUBJECT_LIMIT,
            placeholder: "Why, in more words (optional)",
            earlier,
        }),
        check: Some(Rc::new(check)),
        items_title: "Staged",
        items_summary: staged_summary(&staged),
        items: staged,
        ..Prompt::default()
    };
    editor.read(
        prompt,
        |_, _| Completions::default(),
        move |editor, message| {
            let mut args = vec!["commit"];
            args.extend(sign_off.get().then_some("--signoff"));
            args.extend(amend.get().then_some("--amend"));
            args.extend(skip_hooks.get().then_some("--no-verify"));
            run(editor, &root, commit_job(args, message))
        },
    )
}

fn commit_job(mut args: Vec<&'static str>, message: String) -> Job {
    Box::new(move |root| {
        let mut command = git(root);
        let committed = if message.trim().is_empty() {
            args.push("--no-edit");
            output(command.args(&args))
        } else {
            args.extend(["--file", "-"]);
            output_with_input(command.args(&args), message.as_bytes())
        };
        committed.map(drop).map_err(|error| format!("git commit: {error}"))
    })
}

fn staged_items(snapshot: &Snapshot) -> Vec<ListItem> {
    section_files(snapshot, Section::Staged)
        .into_iter()
        .map(|file| ListItem {
            icon: None,
            mark: None,
            label: file
                .path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
            detail: directory_label(&file.path),
            state: Some(file.state),
            lines: file.lines,
        })
        .collect()
}

/// `2 files · +112 −3`.
fn staged_summary(items: &[ListItem]) -> String {
    let lines =
        items.iter().filter_map(|item| item.lines).fold(LineCounts::default(), |total, lines| {
            LineCounts { added: total.added + lines.added, removed: total.removed + lines.removed }
        });
    let files = match items.len() {
        0 => return "Nothing".into(),
        1 => "1 file".to_owned(),
        count => format!("{count} files"),
    };
    format!("{files} · +{} −{}", lines.added, lines.removed)
}

/// Who commits, as Git would record them.
fn author(root: &Path) -> String {
    let ident = output(git(root).args(["var", "GIT_AUTHOR_IDENT"])).unwrap_or_default();
    let ident = String::from_utf8_lossy(&ident);
    let name = ident.split(" <").next().unwrap_or_default().trim();
    // A backtick would start a quoted name in the prompt's subject.
    short_author(name).replace('`', "'")
}

fn branch(head: &Head) -> String {
    match head {
        Head::Branch(name) | Head::Unborn(name) => quote(name),
        Head::Detached(_) => "detached HEAD".into(),
    }
}

/// The messages of the last commits, newest first.
fn earlier_messages(root: &Path) -> Vec<String> {
    let printed = output(git(root).args(["log", "-z", "--format=%B", "-n", EARLIER_MESSAGES]))
        .unwrap_or_default();
    String::from_utf8_lossy(&printed)
        .split('\0')
        .map(|message| message.trim().to_owned())
        .filter(|message| !message.is_empty())
        .collect()
}
