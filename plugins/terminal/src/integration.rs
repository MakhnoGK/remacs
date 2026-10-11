//! Shell integration for zsh and bash, which report nothing by themselves: their hooks
//! injected as kitty and Ghostty do, after which they read the user's own startup files.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const FILES: &[(&str, &str)] = &[
    (
        "zsh/.zshenv",
        include_str!("../shell-integration/zsh/.zshenv"),
    ),
    (
        "zsh/remacs.zsh",
        include_str!("../shell-integration/zsh/remacs.zsh"),
    ),
    (
        "bash/remacs.bash",
        include_str!("../shell-integration/bash/remacs.bash"),
    ),
];

/// A shell's arguments and environment, changed to start it with the hooks.
pub struct Injected {
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

/// How to start `program` with the hooks; none for shells it has none for, or a bash
/// asked to run a command or to start in posix mode.
///
/// zsh reads `.zshenv` from `ZDOTDIR`, ours, which puts the user's back. bash reads an
/// `--rcfile`, ours, which reads the user's files as bash would have; a login shell
/// reads none, so it starts as another one and the script reads the profile.
pub fn inject(program: &Path, args: &[String], env: &BTreeMap<String, String>) -> Option<Injected> {
    let shell = program.file_name()?.to_str()?;
    let mut env = env.clone();
    let mut args = args.to_vec();
    match shell {
        "zsh" => {
            let directory = written()?.join("zsh");
            if let Some(own) = env
                .get("ZDOTDIR")
                .cloned()
                .or_else(|| env::var("ZDOTDIR").ok())
            {
                env.insert("REMACS_ZSH_ZDOTDIR".to_owned(), own);
            }
            env.insert(
                "ZDOTDIR".to_owned(),
                directory.to_string_lossy().into_owned(),
            );
        }
        "bash" => {
            if args.iter().any(|arg| arg == "-c" || arg == "--posix") {
                return None;
            }
            let script = written()?.join("bash/remacs.bash");
            // bash reads the rcfile alone, and only when not a login shell; the script
            // reads what bash would have, told by these.
            for (flags, name) in [
                (&["--login", "-l"][..], "REMACS_BASH_LOGIN"),
                (&["--noprofile"], "REMACS_BASH_NOPROFILE"),
                (&["--norc"], "REMACS_BASH_NORC"),
            ] {
                let before = args.len();
                args.retain(|arg| !flags.contains(&arg.as_str()));
                if args.len() < before {
                    env.insert(name.to_owned(), "1".to_owned());
                }
            }
            if let Some(index) = args.iter().position(|arg| arg == "--rcfile") {
                let rcfile = args.drain(index..(index + 2).min(args.len())).nth(1);
                env.insert("REMACS_BASH_RCFILE".to_owned(), rcfile.unwrap_or_default());
            }
            args.splice(
                0..0,
                ["--rcfile".to_owned(), script.to_string_lossy().into_owned()],
            );
            env.insert("REMACS_BASH_INJECT".to_owned(), "1".to_owned());
        }
        _ => return None,
    }
    Some(Injected { args, env })
}

/// Where the hooks are, written there first if missing or from another version.
fn written() -> Option<PathBuf> {
    let directory = env::temp_dir().join("remacs-shell-integration");
    write_files(&directory).ok()?;
    Some(directory)
}

fn write_files(directory: &Path) -> io::Result<()> {
    for (name, contents) in FILES {
        let path = directory.join(name);
        if fs::read_to_string(&path).is_ok_and(|current| current == *contents) {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, contents)?;
    }
    Ok(())
}
