//! Programs' versions, for servers with a `min-version`: one found older is passed over
//! for the downloaded copy, like TypeScript 5's `tsc`, which has no `--lsp`. Each program
//! is asked its `--version` once, on a thread of its own.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use remacs_core::Notifier;
use remacs_tools::Program;

/// Ample for any `--version`, Node's start included; one stuck is not waited for longer.
// shortcut: a slower answer counts as none until `lsp-restart`; move it to `lsp.toml` if
// a real program (a cold version-manager shim) ever needs longer.
const TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Per program and project root, its major version once it said it; `None` for one that
/// could not. Per root, as a version manager's shim (mise, asdf) runs the project's own.
#[derive(Default)]
pub(crate) struct Versions(Arc<Mutex<HashMap<(PathBuf, PathBuf), Asked>>>);

enum Asked {
    Asking,
    Answered(Option<u64>),
}

impl Versions {
    /// Whether `program`'s major version, run in `root` as the server is, is at least
    /// `minimum`; `None` until it says, `notifier` being called once it has.
    pub(crate) fn at_least(
        &self,
        program: &Program,
        root: &Path,
        minimum: u64,
        notifier: &Notifier,
    ) -> Option<bool> {
        let key = (program.path.clone(), root.to_path_buf());
        let mut asked = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match asked.get(&key) {
            Some(Asked::Answered(major)) => {
                return Some(major.is_some_and(|major| major >= minimum));
            }
            Some(Asked::Asking) => return None,
            None => {}
        }
        asked.insert(key.clone(), Asked::Asking);
        let (versions, program, notifier) =
            (Arc::clone(&self.0), program.clone(), Arc::clone(notifier));
        thread::spawn(move || {
            let major = major_version(&program, &key.1, TIMEOUT);
            let mut asked = versions.lock().unwrap_or_else(PoisonError::into_inner);
            asked.insert(key, Asked::Answered(major));
            drop(asked);
            notifier();
        });
        None
    }

    /// Asks every program again, e.g. after one was upgraded.
    pub(crate) fn forget(&self) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

/// The major version `program --version` says; `None` for one that fails, says none or
/// has not said by `timeout`, as the server would not start from it either.
fn major_version(program: &Program, root: &Path, timeout: Duration) -> Option<u64> {
    let mut child = Command::new(&program.path)
        .arg("--version")
        .current_dir(root)
        .envs(program.env.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout;
    // A version is a line, well within the pipe's buffer, so the child never waits on it.
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL_INTERVAL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let mut stdout = Vec::new();
    child.stdout.take()?.read_to_end(&mut stdout).ok()?;
    status
        .success()
        .then(|| first_number(&String::from_utf8_lossy(&stdout)))
        .flatten()
}

/// The first number in `text`, as in `Version 5.7.2` or `clangd version 18.1.3`.
fn first_number(text: &str) -> Option<u64> {
    let start = text.find(|c: char| c.is_ascii_digit())?;
    let digits = &text[start..];
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    digits[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_major_version_is_the_first_number_said() {
        assert_eq!(first_number("Version 5.7.2\n"), Some(5));
        assert_eq!(first_number("clangd version 18.1.3"), Some(18));
        assert_eq!(first_number("no version"), None);
    }

    #[test]
    fn a_program_stuck_on_its_version_is_stopped_and_has_none() {
        use std::os::unix::fs::PermissionsExt;

        let path = std::env::temp_dir().join(format!("remacs-stuck-{}", std::process::id()));
        std::fs::write(&path, "#!/bin/sh\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let program = Program {
            path: path.clone(),
            env: Vec::new(),
        };
        let started = Instant::now();
        let root = std::env::temp_dir();
        assert_eq!(
            major_version(&program, &root, Duration::from_millis(200)),
            None
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        std::fs::remove_file(&path).unwrap();
    }
}
