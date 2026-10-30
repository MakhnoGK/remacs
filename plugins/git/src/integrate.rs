//! Bringing another branch's commits in: rebasing onto it, the commits to replay picked,
//! squashed, fixed up, dropped or reordered first, or merging it, like Emacs
//! `magit-rebase-interactive` and `magit-merge`.

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use remacs_core::{
    Candidate, CommandResult, CommitChain, Completions, Count, Editor, Keymap, Prompt,
    PromptOption, RefChip, RefKind, Tone, quote,
};

use crate::command::{git, output};
use crate::commands::{run, shown_status};
use crate::monitor::Job;
use crate::status::{Head, Snapshot};
use crate::status_view::SHORT_ID;

pub const NEXT_TARGET: &str = "git-integrate-next-target";
pub const PICK: &str = "git-step-pick";
pub const SQUASH: &str = "git-step-squash";
pub const FIXUP: &str = "git-step-fixup";
pub const DROP: &str = "git-step-drop";
pub const EARLIER: &str = "git-step-earlier";
pub const LATER: &str = "git-step-later";
const KEYS: &[(&str, &str)] = &[
    ("TAB", NEXT_TARGET),
    ("p", PICK),
    ("s", SQUASH),
    ("f", FIXUP),
    ("d", DROP),
    ("M-<up>", EARLIER),
    ("M-<down>", LATER),
];
const REBASE_HINTS: &[(&str, &str)] = &[
    ("↵", "Start rebase"),
    ("⇥", "Onto"),
    ("M-↑ M-↓", "Reorder"),
    ("p s f d", "Pick · squash · fixup · drop"),
    ("C-g", "Cancel"),
];
const MERGE_HINTS: &[(&str, &str)] = &[("↵", "Merge"), ("⇥", "Branch"), ("C-g", "Cancel")];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Pick,
    Squash,
    Fixup,
    Drop,
}

impl Action {
    fn name(self) -> &'static str {
        match self {
            Action::Pick => "pick",
            Action::Squash => "squash",
            Action::Fixup => "fixup",
            Action::Drop => "drop",
        }
    }

    fn tone(self) -> Tone {
        match self {
            Action::Pick => Tone::Plain,
            Action::Squash | Action::Fixup => Tone::Special,
            Action::Drop => Tone::Danger,
        }
    }

    /// Makes a commit of its own.
    fn keeps_commit(self) -> bool {
        self == Action::Pick
    }
}

/// A commit to replay, and what to do with it.
#[derive(Clone, Debug)]
pub struct Step {
    id: String,
    subject: String,
    action: Action,
}

/// One commit, short.
#[derive(Clone, Debug)]
struct Commit {
    id: String,
    subject: String,
}

/// What the open rebase or merge prompt is about; its keys change it.
pub struct Plan {
    root: PathBuf,
    head: String,
    /// Branches to bring in, the base first.
    targets: Vec<String>,
    target: usize,
    /// Oldest first, as Git replays them.
    steps: Vec<Step>,
    /// On the target and not here.
    incoming: Vec<Commit>,
    /// The steps were changed from picking each in order.
    edited: bool,
}

/// The plan of the open prompt, for its keys.
#[derive(Default)]
pub struct OpenPlan(Option<Rc<RefCell<Plan>>>);

impl Plan {
    fn target(&self) -> &str {
        &self.targets[self.target]
    }

    fn read(&mut self) -> Result<(), String> {
        let target = self.target().to_owned();
        self.steps = log(&self.root, &[&format!("{target}..HEAD"), "--reverse", "--no-merges"])?
            .into_iter()
            .map(|commit| Step { id: commit.id, subject: commit.subject, action: Action::Pick })
            .collect();
        self.incoming = log(&self.root, &[&format!("HEAD..{target}")])?;
        self.edited = false;
        Ok(())
    }

    /// The first step kept must stand alone: there is nothing before it to fold into.
    fn check(&self) -> Result<(), String> {
        let first = self.steps.iter().find(|step| step.action != Action::Drop);
        match first {
            Some(step) if !step.action.keeps_commit() => {
                Err(format!("{} has nothing before it to fold into", short(&step.id)))
            }
            _ => Ok(()),
        }
    }
}

/// Rebases the current branch onto its base, or another branch.
pub fn rebase(editor: &mut Editor) -> CommandResult {
    open(editor, true)
}

/// Merges the base, or another branch, into the current one.
pub fn merge(editor: &mut Editor) -> CommandResult {
    open(editor, false)
}

fn open(editor: &mut Editor, rebasing: bool) -> CommandResult {
    let (root, snapshot) = shown_status(editor)?;
    let head = match &snapshot.head {
        Head::Branch(name) => name.clone(),
        Head::Detached(_) => "HEAD".into(),
        Head::Unborn(_) => return Err("Commit something first".into()),
    };
    let targets = targets(&root, &snapshot, &head)?;
    if targets.is_empty() {
        return Err("No other branch to bring in".into());
    }
    let mut plan = Plan {
        root: root.clone(),
        head: head.clone(),
        targets,
        target: 0,
        steps: Vec::new(),
        incoming: Vec::new(),
        edited: false,
    };
    plan.read()?;
    let initial_target = plan.target().to_owned();
    let plan = Rc::new(RefCell::new(plan));
    editor.plugin_state::<OpenPlan>().0 = Some(plan.clone());

    let rebase = Rc::new(Cell::new(rebasing));
    let [interactive, autostash, autosquash] = [true, true, false].map(|on| Rc::new(Cell::new(on)));
    let option = |key, labels: &[&str], state: &Rc<Cell<bool>>, shown_while| PromptOption {
        key,
        labels: labels.iter().map(|label| (*label).to_owned()).collect(),
        state: state.clone(),
        shown_while,
        hidden_while: None,
    };
    let mut keymap = Keymap::default();
    for &(keys, command) in KEYS {
        keymap.bind(keys, command);
    }
    let changed = changed_files(&snapshot);
    let prompt = Prompt {
        label: "Onto".into(),
        initial_input: initial_target,
        count: Count::Matches,
        wide: true,
        read_only: true,
        keymap: Some(keymap),
        options: vec![
            option('m', &["Merge", "Rebase"], &rebase, None),
            option('i', &["Interactive"], &interactive, Some(rebase.clone())),
            option('a', &["Autostash"], &autostash, None),
            option('q', &["Autosquash"], &autosquash, Some(rebase.clone())),
        ],
        check: Some(Rc::new({
            let (plan, rebase) = (plan.clone(), rebase.clone());
            move |_: &str| {
                let plan = plan.borrow();
                match rebase.get() {
                    true if plan.incoming.is_empty() && !plan.edited => {
                        Err(format!("Already on top of {}", plan.target()))
                    }
                    true => plan.check(),
                    false if plan.incoming.is_empty() => {
                        Err(format!("{} has nothing new", plan.target()))
                    }
                    false => Ok(()),
                }
            }
        })),
        ..Prompt::default()
    };
    let complete = {
        let (plan, rebase, autostash) = (plan.clone(), rebase.clone(), autostash.clone());
        move |_: &Editor, _: &str| {
            completions(&plan.borrow(), rebase.get(), autostash.get(), changed)
        }
    };
    editor.read(prompt, complete, move |editor, _| {
        editor.plugin_state::<OpenPlan>().0 = None;
        let plan = plan.borrow();
        let job = if rebase.get() {
            rebase_job(&plan, interactive.get() && plan.edited, autostash.get(), autosquash.get())
        } else {
            merge_job(plan.target().to_owned(), autostash.get())
        };
        run(editor, &root, job)
    })
}

fn completions(plan: &Plan, rebasing: bool, autostash: bool, changed: usize) -> Completions {
    let target = plan.target();
    let incoming = plan.incoming.len();
    let commit_row =
        |id: &str, subject: &str, action: Option<Action>, group: &'static str| Candidate {
            label: short(id),
            annotation: subject.to_owned(),
            group: Some(group.into()),
            action: action.map(|action| (action.name(), action.tone())),
            ..Candidate::new(id)
        };
    let (candidates, outcome): (Vec<Candidate>, _) = if rebasing {
        let kept = plan.steps.iter().filter(|step| step.action.keeps_commit()).count();
        let rows = plan
            .steps
            .iter()
            .map(|step| commit_row(&step.id, &step.subject, Some(step.action), "Commits to replay"))
            .collect();
        let summary = format!("{} on top of {target}", commits(kept));
        (rows, (summary, chain(target, &plan.head, kept)))
    } else {
        let rows = plan
            .incoming
            .iter()
            .map(|commit| commit_row(&commit.id, &commit.subject, None, "Commits to merge"))
            .collect();
        let (summary, dots) = if plan.steps.is_empty() {
            (format!("{} fast-forwards to {target}", plan.head), 0)
        } else {
            (format!("A merge commit on {}", plan.head), 1)
        };
        (rows, (summary, chain(target, &plan.head, dots)))
    };
    let footnote = (changed > 0).then(|| {
        let files = match changed {
            1 => "1 changed file".to_owned(),
            count => format!("{count} changed files"),
        };
        if autostash {
            format!("{files} will be stashed and restored")
        } else {
            format!("{files}: Git may refuse without Autostash")
        }
    });
    Completions {
        total: candidates.len(),
        candidates,
        selected: None,
        note: Some(match incoming {
            0 => format!("Up to date with {target}"),
            1 => format!("1 new commit on {target}"),
            count => format!("{count} new commits on {target}"),
        }),
        hints: Some(if rebasing { REBASE_HINTS } else { MERGE_HINTS }),
        label: Some(if rebasing { "Onto" } else { "Merge" }.into()),
        subject: Some(if rebasing {
            format!("{} onto {}", quote(&plan.head), quote(target))
        } else {
            format!("{} into {}", quote(target), quote(&plan.head))
        }),
        count_groups: true,
        outcome: Some(outcome),
        footnote,
    }
}

fn chain(target: &str, head: &str, commits: usize) -> CommitChain {
    CommitChain {
        base: RefChip { name: target.into(), kind: RefKind::Main },
        tip: RefChip { name: head.into(), kind: RefKind::Head },
        incoming: 0,
        commits,
        note: None,
    }
}

/// Brings in the next branch of the plan's.
pub fn next_target(editor: &mut Editor) -> CommandResult {
    let plan = open_plan(editor)?;
    let target = {
        let mut plan = plan.borrow_mut();
        plan.target = (plan.target + 1) % plan.targets.len();
        plan.read()?;
        plan.target().to_owned()
    };
    if let Some(minibuffer) = editor.minibuffer_mut() {
        minibuffer.set_input(&target);
    }
    editor.refresh_minibuffer();
    Ok(())
}

pub fn pick(editor: &mut Editor) -> CommandResult {
    set_action(editor, Action::Pick)
}

pub fn squash(editor: &mut Editor) -> CommandResult {
    set_action(editor, Action::Squash)
}

pub fn fixup(editor: &mut Editor) -> CommandResult {
    set_action(editor, Action::Fixup)
}

pub fn drop_step(editor: &mut Editor) -> CommandResult {
    set_action(editor, Action::Drop)
}

pub fn earlier(editor: &mut Editor) -> CommandResult {
    move_step(editor, -1)
}

pub fn later(editor: &mut Editor) -> CommandResult {
    move_step(editor, 1)
}

fn set_action(editor: &mut Editor, action: Action) -> CommandResult {
    let (plan, index) = selected_step(editor)?;
    {
        let mut plan = plan.borrow_mut();
        plan.steps[index].action = action;
        plan.edited = true;
    }
    editor.refresh_minibuffer();
    Ok(())
}

fn move_step(editor: &mut Editor, by: isize) -> CommandResult {
    let (plan, index) = selected_step(editor)?;
    let to = {
        let mut plan = plan.borrow_mut();
        let to = index
            .checked_add_signed(by)
            .filter(|to| *to < plan.steps.len())
            .ok_or("Nothing to move past")?;
        plan.steps.swap(index, to);
        plan.edited = true;
        to
    };
    editor.refresh_minibuffer();
    if let Some(minibuffer) = editor.minibuffer_mut() {
        minibuffer.select(to);
    }
    Ok(())
}

/// The plan and the step selected in it; merging has no steps.
fn selected_step(editor: &mut Editor) -> Result<(Rc<RefCell<Plan>>, usize), &'static str> {
    let plan = open_plan(editor)?;
    let minibuffer = editor.minibuffer().ok_or("Not in the minibuffer")?;
    let is_step =
        minibuffer.selected_candidate().is_some_and(|candidate| candidate.action.is_some());
    let index = minibuffer.selected().filter(|_| is_step).ok_or("No commit to replay here")?;
    Ok((plan, index))
}

fn open_plan(editor: &mut Editor) -> Result<Rc<RefCell<Plan>>, &'static str> {
    editor.plugin_state::<OpenPlan>().0.clone().ok_or("No rebase or merge is being set up")
}

/// Rebases onto the plan's target; edited, its steps are given to Git as the to-do list.
fn rebase_job(plan: &Plan, edited: bool, autostash: bool, autosquash: bool) -> Job {
    let target = plan.target().to_owned();
    let todo: String = plan
        .steps
        .iter()
        .map(|step| format!("{} {} {}\n", step.action.name(), step.id, step.subject))
        .collect();
    Box::new(move |root| {
        let mut command = git(root);
        command.arg("rebase");
        if autostash {
            command.arg("--autostash");
        }
        if autosquash {
            command.arg("--autosquash");
        }
        let todo_path = std::env::temp_dir().join(format!("remacs-rebase-{}", std::process::id()));
        if edited {
            fs::write(&todo_path, &todo).map_err(|error| format!("Writing the steps: {error}"))?;
            // Git hands its to-do list to the sequence editor, which puts the steps there;
            // squashed messages are kept as Git combines them.
            command
                .arg("--interactive")
                .env("GIT_SEQUENCE_EDITOR", format!("cp '{}'", todo_path.display()))
                .env("GIT_EDITOR", "true");
        }
        let rebased =
            output(command.arg(&target)).map(drop).map_err(|error| format!("git rebase: {error}"));
        if edited {
            let _ = fs::remove_file(&todo_path);
        }
        rebased
    })
}

fn merge_job(target: String, autostash: bool) -> Job {
    Box::new(move |root| {
        let mut command = git(root);
        command.args(["merge", "--no-edit"]);
        if autostash {
            command.arg("--autostash");
        }
        output(command.arg(&target)).map(drop).map_err(|error| format!("git merge: {error}"))
    })
}

/// The base first, then the upstream, the other local branches and the remote ones.
fn targets(root: &Path, snapshot: &Snapshot, head: &str) -> Result<Vec<String>, String> {
    let printed = output(git(root).args([
        "for-each-ref",
        "--format=%(refname)",
        "refs/heads",
        "refs/remotes",
    ]))
    .map_err(|error| format!("git for-each-ref: {error}"))?;
    let printed = String::from_utf8_lossy(&printed);
    let branches = printed
        .lines()
        .filter(|name| !(name.starts_with("refs/remotes/") && name.ends_with("/HEAD")))
        .filter_map(|name| {
            name.strip_prefix("refs/heads/").or_else(|| name.strip_prefix("refs/remotes/"))
        })
        .map(str::to_owned);
    let base = snapshot
        .details
        .as_ref()
        .and_then(|details| details.base.as_ref())
        .map(|base| base.name.clone());
    let upstream = snapshot.upstream.as_ref().map(|upstream| upstream.name.clone());
    let mut targets: Vec<String> = Vec::new();
    for name in base.into_iter().chain(upstream).chain(branches) {
        if name != head && !targets.contains(&name) {
            targets.push(name);
        }
    }
    Ok(targets)
}

/// Tracked files with changes, which a rebase or merge may refuse to run over.
fn changed_files(snapshot: &Snapshot) -> usize {
    snapshot.files.values().filter(|entry| entry.staged != b'?').count()
}

fn log(root: &Path, range: &[&str]) -> Result<Vec<Commit>, String> {
    let printed = output(git(root).arg("log").arg("--format=%H%x00%s").args(range).arg("--"))
        .map_err(|error| format!("git log: {error}"))?;
    Ok(String::from_utf8_lossy(&printed)
        .lines()
        .filter_map(|line| {
            let (id, subject) = line.split_once('\0')?;
            Some(Commit { id: id.to_owned(), subject: subject.to_owned() })
        })
        .collect())
}

fn short(id: &str) -> String {
    id.chars().take(SHORT_ID).collect()
}

/// `1 commit`, `3 commits`.
fn commits(count: usize) -> String {
    if count == 1 { "1 commit".into() } else { format!("{count} commits") }
}
