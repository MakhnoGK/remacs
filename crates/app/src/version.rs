//! What `cargo xtask bundle` builds with: the tag's version and the commit. A plain
//! `cargo build` has the workspace's version and no commit.

pub const VERSION: &str = match option_env!("REMACS_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

pub const COMMIT: &str = match option_env!("REMACS_COMMIT") {
    Some(commit) => commit,
    None => "unknown commit",
};
