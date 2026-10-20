//! Pushing the branch to its upstream or elsewhere, and pulling into it by rebase, merge
//! or fast-forward, like Emacs `magit-push` and `magit-pull`.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use remacs_core::{
    BannerPart, Candidate, CommandResult, CommitChain, Completions, Editor, Keymap, ListItem,
    Prompt, PromptChoice, PromptOption, RefChip, RefKind, filter_candidates, quote,
};

use crate::command::{git, output};
use crate::commands::{run_saying, shown_status};
use crate::history_view::{ago, short_author};
use crate::monitor::Job;
use crate::status::{Head, Snapshot};
use crate::status_view::SHORT_ID;

pub const PUSH_UPSTREAM: &str = "git-push-upstream";
pub const PUSH_ELSEWHERE: &str = "git-push-elsewhere";
pub const PULL_REBASE: &str = "git-pull-by-rebase";
pub const PULL_MERGE: &str = "git-pull-by-merge";
const UPSTREAM: &str = "upstream";
const ELSEWHERE: &str = "elsewhere";
const PUSH_HINTS: &[(&str, &str)] = &[("↵", "Push"), ("C-g", "Cancel")];
const PULL_HINTS: &[(&str, &str)] = &[("↵", "Pull"), ("r m", "Rebase · merge"), ("C-g", "Cancel")];
/// Commits listed going out or coming in, at most.
const MAX_LISTED: &str = "50";
const MODES: [&str; 3] = ["Rebase", "Merge", "Fast-forward only"];
const REBASE: usize = 0;
const MERGE: usize = 1;
const FAST_FORWARD: usize = 2;

/// A commit, short.
struct Commit {
    id: String,
    subject: String,
    author: String,
    time: i64,
}

/// The branch checked out, when nothing is in progress.
fn current_branch(snapshot: &Snapshot) -> Result<String, &'static str> {
    if snapshot.operation.is_some() {
        return Err("Finish the rebase or merge first");
    }
    match &snapshot.head {
        Head::Branch(name) => Ok(name.clone()),
        Head::Detached(_) => Err("Not on a branch"),
        Head::Unborn(_) => Err("Commit something first"),
    }
}

/// Pushes the branch: to its upstream, or with `e` to a remote branch read next.
pub fn push(editor: &mut Editor) -> CommandResult {
    let (root, snapshot) = shown_status(editor)?;
    let branch = current_branch(&snapshot)?;
    let (target, note) = match &snapshot.upstream {
        Some(upstream) => (upstream.name.clone(), UPSTREAM),
        None => {
            let remote = remotes(&root)
                .into_iter()
                .next()
                .ok_or("No remote to push to")?;
            (format!("{remote}/{branch}"), "new upstream")
        }
    };
    let range = if snapshot.upstream.is_some() {
        vec!["@{upstream}..HEAD"]
    } else {
        vec!["HEAD", "--not", "--remotes"]
    };
    let outgoing = log(&root, &range);
    let [force, tags, dry_run] = [(); 3].map(|_| Rc::new(Cell::new(false)));
    let mut keymap = Keymap::default();
    keymap.bind("u", PUSH_UPSTREAM);
    keymap.bind("e", PUSH_ELSEWHERE);
    let prompt = Prompt {
        read_only: true,
        from_toolbar: true,
        title: Some((
            format!("Push {}", quote(&branch)),
            plural_commits(outgoing.len()),
        )),
        keymap: Some(keymap),
        hints: PUSH_HINTS,
        options: vec![
            PromptOption::chip('f', "Force with lease", &force),
            PromptOption::chip('t', "Push tags", &tags),
            PromptOption::chip('n', "Dry run", &dry_run),
        ],
        items_title: "Outgoing",
        items_summary: outgoing.len().to_string(),
        items: outgoing.iter().map(commit_item).collect(),
        ..Prompt::default()
    };
    let choices = vec![
        Candidate {
            label: target.clone(),
            annotation: note.into(),
            keys: vec!["u".into()],
            ..Candidate::new(UPSTREAM)
        },
        Candidate {
            verb: Some("Elsewhere…"),
            label: String::new(),
            annotation: "pick remote and branch".into(),
            keys: vec!["e".into()],
            ..Candidate::new(ELSEWHERE)
        },
    ];
    let complete = move |_: &Editor, _: &str| Completions::from(choices.clone());
    let new_upstream = snapshot.upstream.is_none();
    editor.read(prompt, complete, move |editor, answer| {
        let flags = Flags {
            force: force.get(),
            tags: tags.get(),
            dry_run: dry_run.get(),
        };
        if answer == ELSEWHERE {
            return push_elsewhere(editor, branch, flags);
        }
        push_to(editor, &root, &branch, &target, new_upstream, flags)
    })
}

#[derive(Clone, Copy)]
struct Flags {
    force: bool,
    tags: bool,
    dry_run: bool,
}

/// Reads a remote branch, e.g. `origin/topic`, and pushes there.
fn push_elsewhere(editor: &mut Editor, branch: String, flags: Flags) -> CommandResult {
    let (root, _) = shown_status(editor)?;
    let known = remotes(&root);
    let branches: Vec<Candidate> = remote_branches(&root)
        .into_iter()
        .map(|name| Candidate {
            annotation: "remote".into(),
            ..Candidate::new(name)
        })
        .collect();
    let default = known.first().map(|remote| format!("{remote}/{branch}"));
    let prompt = Prompt {
        label: "Push to".into(),
        placeholder: "remote/branch",
        initial_input: default.clone().unwrap_or_default(),
        hints: &[("↵", "Push"), ("M-↵", "As typed"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    let complete = move |_: &Editor, input: &str| filter_candidates(input, branches.clone(), false);
    editor.read(prompt, complete, move |editor, answer| {
        if !known
            .iter()
            .any(|remote| answer.starts_with(&format!("{remote}/")))
        {
            return Err(format!("No remote for {answer}").into());
        }
        push_to(editor, &root, &branch, &answer, false, flags)
    })
}

/// Pushes the branch to `target`, a remote branch like `origin/main`, making it the
/// upstream if `new_upstream`.
fn push_to(
    editor: &mut Editor,
    root: &Path,
    branch: &str,
    target: &str,
    new_upstream: bool,
    flags: Flags,
) -> CommandResult {
    let (remote, remote_branch) = target.split_once('/').ok_or("Push to remote/branch")?;
    let mut args: Vec<String> = vec!["push".into()];
    for (on, flag) in [
        (new_upstream, "--set-upstream"),
        (flags.force, "--force-with-lease"),
        (flags.tags, "--tags"),
        (flags.dry_run, "--dry-run"),
    ] {
        if on {
            args.push(flag.into());
        }
    }
    args.push(remote.into());
    args.push(format!("HEAD:refs/heads/{remote_branch}"));
    let done = if flags.dry_run {
        format!("Dry run: {branch} would go to {target}")
    } else {
        format!("Pushed {branch} to {target}")
    };
    run_saying(
        editor,
        root,
        git_job(args, "git push"),
        format!("Pushing {branch} to {target}…"),
        done,
    )
}

/// Pulls into the branch from its upstream by rebase, merge or fast-forward, as
/// configured for it; `r` and `m` switch.
pub fn pull(editor: &mut Editor) -> CommandResult {
    let (root, snapshot) = shown_status(editor)?;
    let branch = current_branch(&snapshot)?;
    let upstream = snapshot
        .upstream
        .clone()
        .ok_or("No upstream to pull from: push with u first")?;
    let (ahead, behind) = upstream
        .ahead_behind
        .ok_or("The upstream is gone from the remote")?;
    let incoming = log(&root, &["HEAD..@{upstream}"]);
    let base: String = text(&root, &["merge-base", "HEAD", "@{upstream}"])
        .unwrap_or_default()
        .chars()
        .take(SHORT_ID)
        .collect();
    let mode = Rc::new(Cell::new(configured_mode(&root, &branch)));
    let [autostash, remember] = [true, false].map(|on| Rc::new(Cell::new(on)));
    let mut summary = Vec::new();
    if ahead > 0 {
        summary.push(BannerPart::Chip(chip(
            &format!("↑{ahead} yours"),
            RefKind::Head,
        )));
    }
    if behind > 0 {
        summary.push(BannerPart::Chip(chip(
            &format!("↓{behind} incoming"),
            RefKind::Main,
        )));
    }
    summary.push(BannerPart::Word(
        match (ahead, behind) {
            (0, 0) => "Up to date with the last fetch",
            (0, _) => "Behind: a fast-forward brings it in",
            (_, 0) => "Nothing new since the last fetch",
            _ => "The branches have diverged",
        }
        .into(),
    ));
    let mut keymap = Keymap::default();
    keymap.bind("r", PULL_REBASE);
    keymap.bind("m", PULL_MERGE);
    let diverged = ahead > 0 && behind > 0;
    let prompt = Prompt {
        read_only: true,
        from_toolbar: true,
        title: Some((
            format!("Pull into {}", quote(&branch)),
            quote(&upstream.name),
        )),
        summary,
        keymap: Some(keymap),
        hints: PULL_HINTS,
        choices: vec![PromptChoice {
            label: "Pull by",
            key: "TAB",
            values: MODES.iter().map(|mode| (*mode).to_owned()).collect(),
            selected: mode.clone(),
            segmented: true,
        }],
        options: vec![
            PromptOption::chip('a', "Autostash", &autostash),
            PromptOption::chip('b', "Remember for this branch", &remember),
        ],
        items_title: "Incoming",
        items_summary: incoming.len().to_string(),
        items: incoming.iter().map(commit_item).collect(),
        check: Some(Rc::new({
            let mode = mode.clone();
            move |_: &str| match mode.get() {
                FAST_FORWARD if diverged => {
                    Err("Fast-forward isn't possible: the branches have diverged".into())
                }
                _ => Ok(()),
            }
        })),
        ..Prompt::default()
    };
    let head = branch.clone();
    let complete = {
        let mode = mode.clone();
        move |_: &Editor, _: &str| Completions {
            outcome: Some((
                String::new(),
                pull_chain(&base, &head, ahead, behind, mode.get()),
            )),
            ..Completions::default()
        }
    };
    editor.read(prompt, complete, move |editor, _| {
        let mode = mode.get();
        let mut args: Vec<String> = vec!["pull".into()];
        args.push(
            match mode {
                REBASE => "--rebase",
                MERGE => "--no-rebase",
                _ => "--ff-only",
            }
            .into(),
        );
        if mode == MERGE {
            args.push("--no-edit".into());
        }
        if autostash.get() {
            args.push("--autostash".into());
        }
        let remember = (remember.get() && mode != FAST_FORWARD).then(|| {
            (
                format!("branch.{branch}.rebase"),
                (mode == REBASE).to_string(),
            )
        });
        let pull = git_job(args, "git pull");
        let job: Job = Box::new(move |root| {
            if let Some((key, value)) = &remember {
                output(git(root).args(["config", key, value]))
                    .map_err(|error| format!("git config: {error}"))?;
            }
            pull(root)
        });
        run_saying(
            editor,
            &root,
            job,
            format!("Pulling into {branch}…"),
            format!("Pulled {} into {branch}", upstream.name),
        )
    })
}

/// What the branch looks like after pulling: the commits come in after the merge base,
/// yours after them.
fn pull_chain(base: &str, head: &str, ahead: u64, behind: u64, mode: usize) -> CommitChain {
    let (incoming, commits) = (behind as usize, ahead as usize);
    let incoming_ones = match incoming {
        1 => "one".to_owned(),
        count => count.to_string(),
    };
    let (commits, note) = match mode {
        _ if behind == 0 => (
            commits,
            "Nothing new since the last fetch; pulling fetches first.".to_owned(),
        ),
        _ if ahead == 0 => (
            0,
            format!("{head} fast-forwards to the incoming {incoming_ones}."),
        ),
        REBASE => {
            let yours = match commits {
                1 => "Your commit is".to_owned(),
                count => format!("Your {count} commits are"),
            };
            (
                commits,
                format!(
                    "{yours} replayed on top of the incoming {incoming_ones}. History stays linear."
                ),
            )
        }
        MERGE => (
            commits + 1,
            format!(
                "A merge commit joins your {} and the incoming {incoming_ones}.",
                plural_commits(commits)
            ),
        ),
        _ => (
            commits,
            "Fast-forward isn't possible while the branches have diverged.".into(),
        ),
    };
    CommitChain {
        base: chip(base, RefKind::Remote),
        tip: chip(head, RefKind::Head),
        incoming,
        commits,
        note: Some(note),
    }
}

/// `branch.<name>.rebase`, else `pull.rebase`; rebasing when neither says.
fn configured_mode(root: &Path, branch: &str) -> usize {
    let configured = text(
        root,
        &["config", "--get", &format!("branch.{branch}.rebase")],
    )
    .or_else(|| text(root, &["config", "--get", "pull.rebase"]));
    match configured.as_deref() {
        Some("false") => MERGE,
        _ => REBASE,
    }
}

/// Picks the pull's rebase.
pub fn by_rebase(editor: &mut Editor) -> CommandResult {
    set_mode(editor, REBASE)
}

/// Picks the pull's merge.
pub fn by_merge(editor: &mut Editor) -> CommandResult {
    set_mode(editor, MERGE)
}

fn set_mode(editor: &mut Editor, mode: usize) -> CommandResult {
    let minibuffer = editor.minibuffer().ok_or("Not in the minibuffer")?;
    let choice = minibuffer
        .prompt()
        .choices
        .first()
        .ok_or("Nothing to choose")?;
    choice.selected.set(mode);
    editor.refresh_minibuffer();
    Ok(())
}

/// Chooses the upstream, as `u` does.
pub fn upstream(editor: &mut Editor) -> CommandResult {
    choose(editor, 0)
}

/// Chooses another remote branch, as `e` does.
pub fn elsewhere(editor: &mut Editor) -> CommandResult {
    choose(editor, 1)
}

fn choose(editor: &mut Editor, index: usize) -> CommandResult {
    editor
        .minibuffer_mut()
        .ok_or("Not in the minibuffer")?
        .select(index);
    editor.exit_minibuffer(false)
}

fn chip(name: &str, kind: RefKind) -> RefChip {
    RefChip {
        name: name.into(),
        kind,
    }
}

fn commit_item(commit: &Commit) -> ListItem {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    ListItem {
        icon: None,
        mark: None,
        label: format!("{}  {}", commit.id, commit.subject),
        detail: format!(
            "{} · {}",
            short_author(&commit.author),
            ago(now - commit.time)
        ),
        state: None,
        lines: None,
    }
}

/// `2 commits`.
fn plural_commits(count: usize) -> String {
    if count == 1 {
        "1 commit".into()
    } else {
        format!("{count} commits")
    }
}

fn log(root: &Path, range: &[&str]) -> Vec<Commit> {
    let printed = output(
        git(root)
            .args(["log", "--format=%h%x00%s%x00%an%x00%at", "-n", MAX_LISTED])
            .args(range)
            .arg("--"),
    )
    .unwrap_or_default();
    String::from_utf8_lossy(&printed)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\0');
            Some(Commit {
                id: fields.next()?.to_owned(),
                subject: fields.next()?.to_owned(),
                author: fields.next()?.to_owned(),
                time: fields.next()?.parse().unwrap_or_default(),
            })
        })
        .collect()
}

fn remotes(root: &Path) -> Vec<String> {
    text(root, &["remote"])
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn remote_branches(root: &Path) -> Vec<String> {
    text(
        root,
        &["for-each-ref", "--format=%(refname:short)", "refs/remotes"],
    )
    .unwrap_or_default()
    .lines()
    .filter(|name| !name.ends_with("/HEAD") && name.contains('/'))
    .map(str::to_owned)
    .collect()
}

fn text(root: &Path, args: &[&str]) -> Option<String> {
    let printed = output(git(root).args(args)).ok()?;
    let printed = String::from_utf8_lossy(&printed).trim().to_owned();
    (!printed.is_empty()).then_some(printed)
}

fn git_job(args: Vec<String>, name: &'static str) -> Job {
    Box::new(move |root| {
        output(git(root).args(&args).env("GIT_EDITOR", "true"))
            .map(drop)
            .map_err(|error| format!("{name}: {error}"))
    })
}
