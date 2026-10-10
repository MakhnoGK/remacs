//! Started from Finder or the Dock, remacs gets the short `PATH` macOS gives apps, so
//! language servers, `cargo` and `git` are not found; it takes the environment the
//! user's login shell sets up instead, as a terminal would have it.

use std::env;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use remacs_config::Section;
use remacs_terminal::find_program;
use serde::Deserialize;

/// Printed before the environment, so that whatever the shell's startup files print is
/// told apart from it.
const MARKER: &str = "__remacs_environment__";
/// Set by the shell itself, or by a terminal, not part of what the user set up.
const SKIPPED: &[&str] = &[
    "_",
    "SHLVL",
    "PWD",
    "OLDPWD",
    "TERM",
    "TERM_PROGRAM",
    "COLUMNS",
    "LINES",
];
const FALLBACK_SHELL: &str = "/bin/zsh";

/// `environment.toml`.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct EnvironmentSettings {
    pub from_login_shell: FromLoginShell,
    /// The shell whose environment is taken, a path or a name, in place of the account's
    /// login shell, e.g. when terminals run another one.
    pub shell: String,
    /// Milliseconds the shell may take to start.
    pub timeout: u64,
}

impl Default for EnvironmentSettings {
    fn default() -> Self {
        Self {
            from_login_shell: FromLoginShell::Auto,
            shell: String::new(),
            timeout: 5000,
        }
    }
}

impl Section for EnvironmentSettings {
    const NAME: &str = "environment";

    fn check(&self) -> Result<(), String> {
        if self.timeout == 0 {
            return Err("timeout is 0".to_owned());
        }
        if !self.shell.is_empty() && find_program(&self.shell).is_none() {
            return Err(format!("shell {:?} is not found", self.shell));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FromLoginShell {
    /// Unless started from a terminal, whose environment it already has.
    Auto,
    Always,
    Never,
}

/// Takes the login shell's environment when the settings ask for it, and starts in the
/// home directory rather than `/`. Must run before any other thread starts.
pub fn import(settings: &EnvironmentSettings) -> Result<(), String> {
    let wanted = match settings.from_login_shell {
        FromLoginShell::Auto => env::var_os("TERM").is_none(),
        FromLoginShell::Always => true,
        FromLoginShell::Never => false,
    };
    if !wanted {
        return Ok(());
    }
    if env::current_dir().is_ok_and(|directory| directory == Path::new("/"))
        && let Some(home) = env::var_os("HOME")
    {
        let _ = env::set_current_dir(home);
    }
    let configured = Some(settings.shell.as_str())
        .filter(|shell| !shell.is_empty())
        .and_then(find_program)
        .map(|path| path.to_string_lossy().into_owned());
    let shell = configured
        .clone()
        .or_else(|| env::var("SHELL").ok())
        .unwrap_or_else(|| FALLBACK_SHELL.to_owned());
    let variables = run_shell(&shell, Duration::from_millis(settings.timeout))?;
    let variables = variables
        .split('\0')
        .filter_map(|variable| variable.split_once('='))
        .filter(|(name, _)| !name.is_empty() && !SKIPPED.contains(name))
        // The shell inherits `SHELL`; the configured one is what terminals should run.
        .chain(configured.as_deref().map(|shell| ("SHELL", shell)));
    for (name, value) in variables {
        // SAFETY: called from `main` before the app or any plugin starts a thread; the
        // reader above has sent what it read and touches no environment.
        unsafe { env::set_var(name, value) };
    }
    Ok(())
}

/// The environment an interactive login `shell` prints, given `timeout` to do it. Read up
/// to the end marker rather than the end of output, which a daemon the startup files
/// start (e.g. ssh-agent) may hold open.
fn run_shell(shell: &str, timeout: Duration) -> Result<String, String> {
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c"])
        .arg(format!(
            "printf '%s' {MARKER}; /usr/bin/env -0; printf '%s' {MARKER}"
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("{shell}: {error}"))?;
    let mut stdout = child.stdout.take().expect("piped");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut output = Vec::new();
        let mut chunk = [0; 8192];
        while let Ok(read @ 1..) = stdout.read(&mut chunk) {
            output.extend_from_slice(&chunk[..read]);
            let text = String::from_utf8_lossy(&output);
            if let Some((_, after)) = text.split_once(MARKER)
                && let Some((variables, _)) = after.split_once(MARKER)
            {
                let _ = sender.send(variables.to_owned());
                return;
            }
        }
    });
    let variables = receiver.recv_timeout(timeout);
    let _ = child.kill();
    let _ = child.wait();
    variables.map_err(|error| match error {
        RecvTimeoutError::Timeout => format!(
            "{shell} took longer than {} ms to start",
            timeout.as_millis()
        ),
        RecvTimeoutError::Disconnected => format!("{shell} printed no environment"),
    })
}
