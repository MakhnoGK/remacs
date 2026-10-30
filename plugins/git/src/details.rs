//! What a status buffer shows above and beside the files: the last commit's subject, the
//! branch the current one is based on, and the stashes.

use std::path::Path;

use crate::command::{git, output};
use crate::status::{Head, Snapshot};

/// Per-branch settings naming its base, ours first: GitHub CLI's and VS Code's are read
/// too, so a base set there carries over.
const BASE_KEYS: [&str; 3] = [BASE_KEY, "gh-merge-base", "vscode-merge-base"];
/// Where a branch created here records its base: `branch.<name>.remacs-base`.
pub const BASE_KEY: &str = "remacs-base";
/// Taken as the base, in this order, when nothing names one and the remote has no
/// default branch.
const USUAL_BASES: [&str; 2] = ["main", "master"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Details {
    /// Of the commit checked out.
    pub subject: Option<String>,
    pub base: Option<Base>,
    pub stashes: Vec<Stash>,
}

/// The branch the current one is based on and will be merged into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Base {
    pub name: String,
    /// On the base and not on the current branch.
    pub new_commits: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stash {
    /// `stash@{0}`
    pub name: String,
    /// Of its commit, which stays the same as stashes come and go.
    pub id: String,
    pub message: String,
}

pub fn read(root: &Path, snapshot: &Snapshot) -> Details {
    let subject = (!snapshot.is_unborn())
        .then(|| text(root, &["log", "-1", "--format=%s", "HEAD"]))
        .flatten();
    let base = match &snapshot.head {
        Head::Branch(branch) => base(root, branch, snapshot),
        _ => None,
    };
    let stashes = if snapshot.stashes > 0 { stashes(root) } else { Vec::new() };
    Details { subject, base, stashes }
}

/// The base named for `branch`, else the remote's default branch, else `main` or
/// `master`; none on the base itself.
fn base(root: &Path, branch: &str, snapshot: &Snapshot) -> Option<Base> {
    let local = text(root, &["for-each-ref", "--format=%(refname:short)", "refs/heads"])
        .unwrap_or_default();
    let has_local = |name: &str| local.lines().any(|line| line == name);
    let remote = snapshot
        .upstream
        .as_ref()
        .and_then(|upstream| upstream.name.split_once('/'))
        .map_or("origin", |(remote, _)| remote);
    let name = configured_base(root, branch)
        .or_else(|| {
            let target = text(
                root,
                &["symbolic-ref", "-q", "--short", &format!("refs/remotes/{remote}/HEAD")],
            )?;
            let default = target.strip_prefix(&format!("{remote}/"))?;
            Some(if has_local(default) { default.to_owned() } else { target.clone() })
        })
        .or_else(|| USUAL_BASES.into_iter().find(|name| has_local(name)).map(str::to_owned))?;
    if name == branch {
        return None;
    }
    let new_commits =
        text(root, &["rev-list", "--count", &format!("HEAD..{name}"), "--"])?.parse().ok()?;
    Some(Base { name, new_commits })
}

fn configured_base(root: &Path, branch: &str) -> Option<String> {
    BASE_KEYS
        .iter()
        .find_map(|key| text(root, &["config", "--get", &format!("branch.{branch}.{key}")]))
}

fn stashes(root: &Path) -> Vec<Stash> {
    text(root, &["stash", "list", "--format=%gd%x00%H%x00%s"])
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\0');
            Some(Stash {
                name: fields.next()?.to_owned(),
                id: fields.next()?.to_owned(),
                message: fields.next()?.to_owned(),
            })
        })
        .collect()
}

/// What the command prints, trimmed; `None` when it fails or prints nothing.
fn text(root: &Path, args: &[&str]) -> Option<String> {
    let printed = output(git(root).args(args)).ok()?;
    let printed = String::from_utf8_lossy(&printed).trim().to_owned();
    (!printed.is_empty()).then_some(printed)
}
