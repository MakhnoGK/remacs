//! A rebase or merge in progress, as Git leaves it in its directory when it stops, e.g.
//! at a conflict.

use std::fs;
use std::path::Path;

use crate::command::{git, output};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operation {
    pub kind: OperationKind,
    /// The branch rebased or merged into, e.g. `feature`; `None` when detached.
    pub branch: Option<String>,
    /// What it is rebased onto, or what is merged in.
    pub other: Commit,
    /// The branch's commit before it began, which aborting goes back to.
    pub original: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperationKind {
    Rebase {
        /// Counted from 1, of `total`.
        step: usize,
        total: usize,
        /// The commit being applied.
        applying: Option<Commit>,
        /// What comes after it, e.g. `squash`.
        next: Option<(String, Commit)>,
    },
    Merge,
}

/// A commit, its id short.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Commit {
    /// The branch at it, if one is.
    pub name: Option<String>,
    pub id: String,
    pub subject: String,
}

impl Commit {
    /// Its branch, else its id.
    pub fn label(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

/// What is in progress in the repository whose Git directory is `git_directory`.
pub fn read(root: &Path, git_directory: &Path) -> Option<Operation> {
    let file = |name: &str| {
        fs::read_to_string(git_directory.join(name))
            .ok()
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
    };
    let rebase = ["rebase-merge", "rebase-apply"]
        .into_iter()
        .find(|directory| git_directory.join(directory).is_dir());
    if let Some(directory) = rebase {
        let within = |name: &str| file(&format!("{directory}/{name}"));
        let number = |names: [&str; 2]| {
            names
                .into_iter()
                .find_map(|name| within(name)?.parse::<usize>().ok())
                .unwrap_or_default()
        };
        let next = within("git-rebase-todo").and_then(|todo| {
            let line = todo
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && !line.starts_with('#'))?
                .to_owned();
            let mut words = line.split_whitespace();
            let action = words.next()?.to_owned();
            Some((action, commit(root, words.next()?)?))
        });
        return Some(Operation {
            kind: OperationKind::Rebase {
                step: number(["msgnum", "next"]),
                total: number(["end", "last"]),
                applying: file("REBASE_HEAD").and_then(|id| commit(root, &id)),
                next,
            },
            branch: within("head-name").map(|name| short_branch(&name)),
            other: within("onto").and_then(|id| commit(root, &id))?,
            original: within("orig-head").map(|id| id.chars().take(7).collect()),
        });
    }
    let merging = file("MERGE_HEAD")?;
    Some(Operation {
        kind: OperationKind::Merge,
        branch: file("HEAD").and_then(|head| head.strip_prefix("ref: ").map(short_branch)),
        other: commit(root, merging.lines().next()?)?,
        original: None,
    })
}

fn short_branch(name: &str) -> String {
    name.strip_prefix("refs/heads/").unwrap_or(name).to_owned()
}

/// The commit `revision` names, with a branch at it, local ones first.
fn commit(root: &Path, revision: &str) -> Option<Commit> {
    let printed =
        output(git(root).args(["log", "-1", "--format=%h%x00%s", revision, "--"])).ok()?;
    let printed = String::from_utf8_lossy(&printed);
    let (id, subject) = printed.trim_end().split_once('\0')?;
    let branches = output(git(root).args([
        "for-each-ref",
        "--format=%(refname:short)",
        "--points-at",
        revision,
        "refs/heads",
        "refs/remotes",
    ]))
    .unwrap_or_default();
    let branches = String::from_utf8_lossy(&branches);
    Some(Commit {
        name: branches
            .lines()
            .find(|name| !name.ends_with("/HEAD"))
            .map(str::to_owned),
        id: id.to_owned(),
        subject: subject.to_owned(),
    })
}
