//! Switching to a branch, or creating one that remembers its base, like Emacs
//! `magit-branch-or-checkout`.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use remacs_core::{
    Candidate, CommandResult, Completions, Count, Editor, Icon, Prompt, PromptChoice, PromptOption,
    filter_candidates,
};

use crate::command::{git, output};
use crate::commands::{git_job, run, shown_status};
use crate::details::BASE_KEY;
use crate::monitor::Job;
use crate::status::Head;
use crate::status_view::SHORT_ID;

const LOCAL: &str = "refs/heads/";
const REMOTE: &str = "refs/remotes/";
const CATEGORY: &str = "branch";
const SWITCH_HINTS: &[(&str, &str)] = &[("↵", "Switch"), ("⇥", "Change base"), ("C-g", "Cancel")];
const CREATE_HINTS: &[(&str, &str)] = &[("↵", "Create"), ("⇥", "Change base"), ("C-g", "Cancel")];

/// A local or remote branch.
struct Branch {
    /// E.g. `refs/heads/main`.
    name: String,
    /// E.g. `main`, `origin/main`.
    short: String,
    id: String,
    /// How far it is from its upstream, e.g. `[ahead 2, behind 1]`.
    track: String,
}

impl Branch {
    fn is_local(&self) -> bool {
        self.name.starts_with(LOCAL)
    }
}

/// Reads a branch name: an existing branch is switched to, a remote one through a local
/// branch tracking it, a new one is created from the base chosen and remembers it.
pub fn branch(editor: &mut Editor) -> CommandResult {
    let (root, snapshot) = shown_status(editor)?;
    if snapshot.is_unborn() {
        return Err("Commit something first".into());
    }
    let branches = Rc::new(read_branches(&root)?);
    let current = match &snapshot.head {
        Head::Branch(name) => Some(name.clone()),
        _ => None,
    };
    let status_base = snapshot
        .details
        .as_ref()
        .and_then(|details| details.base.as_ref())
        .map(|base| base.name.clone());
    let mut bases: Vec<String> = Vec::new();
    for base in [current.clone().unwrap_or_else(|| "HEAD".into())]
        .into_iter()
        .chain(status_base)
        .chain(branches.iter().map(|branch| branch.short.clone()))
    {
        if !bases.contains(&base) {
            bases.push(base);
        }
    }
    let head_id = match &snapshot.head {
        Head::Detached(id) => id.chars().take(SHORT_ID).collect(),
        _ => String::new(),
    };
    let base = Rc::new(Cell::new(0));
    let [switch, track] = [true, false].map(|on| Rc::new(Cell::new(on)));
    let option = |key, label: &str, state: &Rc<Cell<bool>>| PromptOption {
        key,
        labels: vec![label.into()],
        state: state.clone(),
        shown_while: None,
    };
    let prompt = Prompt {
        label: "Branch".into(),
        category: CATEGORY,
        placeholder: "A new or existing branch",
        count: Count::Matches,
        wide: true,
        hints: SWITCH_HINTS,
        choices: vec![PromptChoice {
            label: "Base",
            key: "TAB",
            values: bases.clone(),
            selected: base.clone(),
        }],
        options: vec![
            option('s', "Switch to new branch", &switch),
            option('t', "Track upstream", &track),
        ],
        ..Prompt::default()
    };
    let listed = branches.clone();
    let chosen_base = base.clone();
    let base_names = bases.clone();
    let complete = move |_: &Editor, input: &str| {
        let base = &base_names[chosen_base.get()];
        let base_id = listed
            .iter()
            .find(|branch| branch.short == *base)
            .map_or(head_id.as_str(), |branch| branch.id.as_str());
        completions(&listed, current.as_deref(), base, base_id, input)
    };
    editor.read(prompt, complete, move |editor, answer| {
        let job = if let Some(local) = answer.strip_prefix(LOCAL) {
            git_job(["switch", local])
        } else if let Some(remote) = answer.strip_prefix(REMOTE) {
            let local = remote.split_once('/').map_or(remote, |(_, name)| name);
            let exists = branches
                .iter()
                .any(|branch| branch.is_local() && branch.short == local);
            if exists {
                git_job(["switch", local])
            } else {
                git_job(["switch", "--track", remote])
            }
        } else {
            let base = bases[base.get()].clone();
            create_job(answer, base, switch.get(), track.get())
        };
        run(editor, &root, job)
    })
}

/// A new branch, if `input` names none, then the branches matching it.
fn completions(
    branches: &[Branch],
    current: Option<&str>,
    base: &str,
    base_id: &str,
    input: &str,
) -> Completions {
    let candidates = branches
        .iter()
        .map(|branch| {
            let local = branch.is_local();
            Candidate {
                label: branch.short.clone(),
                annotation: if local {
                    track_note(&branch.track)
                } else {
                    "remote".into()
                },
                tag: (local && Some(branch.short.as_str()) == current).then_some("current"),
                group: Some("Matching branches".into()),
                icon: Some(if local { Icon::Branch } else { Icon::Remote }),
                ..Candidate::new(branch.name.clone())
            }
        })
        .collect();
    let mut completions = filter_candidates(input, candidates, false);
    let input = input.trim();
    // A remote branch of that name is switched to through a local one tracking it.
    let exists = branches.iter().any(|branch| {
        let name = match branch.is_local() {
            true => branch.short.as_str(),
            false => branch.short.split_once('/').map_or("", |(_, name)| name),
        };
        name == input
    });
    let creates = !input.is_empty() && !exists;
    if creates {
        completions.candidates.insert(
            0,
            Candidate {
                verb: Some("Create"),
                annotation: format!("from {base} · {base_id}"),
                keys: vec!["↵".into()],
                icon: Some(Icon::Create),
                ..Candidate::new(input)
            },
        );
        completions.total += 1;
    }
    completions.note = Some("New or existing".into());
    completions.hints = Some(if creates { CREATE_HINTS } else { SWITCH_HINTS });
    completions
}

/// `↑2 to push`, `↓1 to pull`, `↑2 ↓1`, from `[ahead 2, behind 1]`.
fn track_note(track: &str) -> String {
    let count = |word: &str| {
        track
            .trim_matches(['[', ']'])
            .split(", ")
            .find_map(|part| part.strip_prefix(word)?.trim().parse::<u64>().ok())
    };
    match (count("ahead"), count("behind")) {
        _ if track == "[gone]" => "upstream gone".into(),
        (Some(ahead), None) => format!("↑{ahead} to push"),
        (None, Some(behind)) => format!("↓{behind} to pull"),
        (Some(ahead), Some(behind)) => format!("↑{ahead} ↓{behind}"),
        (None, None) => String::new(),
    }
}

/// Creates `name` from `base`, recording the base unless it is a bare commit, and
/// switches to it if asked.
fn create_job(name: String, base: String, switch: bool, track: bool) -> Job {
    Box::new(move |root| {
        let git_run = |args: &[&str]| {
            output(git(root).args(args))
                .map(drop)
                .map_err(|error| format!("git {}: {error}", args[0]))
        };
        let tracking = if track { "--track" } else { "--no-track" };
        git_run(&["branch", tracking, &name, &base])?;
        if base != "HEAD" {
            git_run(&["config", &format!("branch.{name}.{BASE_KEY}"), &base])?;
        }
        if switch {
            git_run(&["switch", &name])?;
        }
        Ok(())
    })
}

fn read_branches(root: &Path) -> Result<Vec<Branch>, String> {
    let printed = output(git(root).args([
        "for-each-ref",
        "--format=%(refname)%00%(objectname:short)%00%(upstream:track)",
        LOCAL,
        REMOTE,
    ]))
    .map_err(|error| format!("git for-each-ref: {error}"))?;
    Ok(String::from_utf8_lossy(&printed)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\0');
            let name = fields.next()?.to_owned();
            if name.starts_with(REMOTE) && name.ends_with("/HEAD") {
                return None;
            }
            let short = name
                .strip_prefix(LOCAL)
                .or_else(|| name.strip_prefix(REMOTE))?
                .to_owned();
            Some(Branch {
                short,
                id: fields.next()?.to_owned(),
                track: fields.next().unwrap_or_default().to_owned(),
                name,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::track_note;

    #[test]
    fn tracking_says_what_to_push_and_pull() {
        assert_eq!(track_note("[ahead 2]"), "↑2 to push");
        assert_eq!(track_note("[behind 1]"), "↓1 to pull");
        assert_eq!(track_note("[ahead 2, behind 1]"), "↑2 ↓1");
        assert_eq!(track_note("[gone]"), "upstream gone");
        assert_eq!(track_note(""), "");
    }
}
