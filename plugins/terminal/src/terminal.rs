//! A terminal buffer's state: its shell's session, how it goes, and what it is called.

use std::env;
use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::time::Instant;

use alacritty_terminal::term::TermMode;
use remacs_core::{Notifier, TabState};
use remacs_files::{abbreviate_home, expand_home};

use crate::copy::CopyMode;
use crate::history::Commands;
use crate::integration::inject;
use crate::links::ScreenLink;
use crate::mouse::MouseState;
use crate::session::{Launch, Phase, Session, lock};
use crate::settings::TerminalSettings;

/// Where shells usually are, for when remacs was started without a login shell's `PATH`,
/// e.g. from the Finder.
const SHELL_DIRECTORIES: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/bin", "/usr/bin"];
/// Characters of a title shown in a chip and a buffer name.
const MAX_TITLE: usize = 28;
/// Started in place of a shell that could not be.
pub const FALLBACK_SHELL: &str = if cfg!(target_os = "macos") { "/bin/zsh" } else { "/bin/sh" };

/// Kept in a terminal's buffer.
pub struct Terminal {
    pub id: u64,
    pub session: Option<Session>,
    pub phase: Phase,
    /// The shell asked for: as configured, or found.
    pub program: String,
    pub args: Vec<String>,
    /// Where the shell is, as it last reported, else where it started.
    pub directory: PathBuf,
    /// Set by the program running.
    pub title: Option<String>,
    /// Given by a rename; wins over the title.
    pub name: Option<String>,
    /// Something was printed since the terminal was last shown.
    pub activity: bool,
    /// The program rang the bell: since the terminal was last shown, and when.
    pub bell: bool,
    pub rang: Option<Instant>,
    pub commands: Commands,
    pub copy: Option<CopyMode>,
    /// Under the mouse while ⌘ is held.
    pub link: Option<ScreenLink>,
    pub mouse: MouseState,
    /// Had the keys when last looked, to tell programs that ask when that changes.
    pub focused: bool,
}

/// What the threads reported, for the plugin to act on.
#[derive(Default)]
pub struct Pumped {
    pub clipboard: Option<String>,
    pub exited: bool,
    pub rang: bool,
}

impl Terminal {
    pub fn new(id: u64, program: String, args: Vec<String>, directory: PathBuf) -> Self {
        Self {
            id,
            session: None,
            phase: Phase::Starting,
            program,
            args,
            directory,
            title: None,
            name: None,
            activity: false,
            bell: false,
            rang: None,
            commands: Commands::default(),
            copy: None,
            link: None,
            mouse: MouseState::default(),
            focused: false,
        }
    }

    /// Starts the shell, again after it ended; a shell that cannot start leaves the
    /// terminal failed.
    pub fn start(&mut self, settings: &TerminalSettings, notifier: Notifier) {
        self.session = None;
        self.copy = None;
        self.commands = Commands::default();
        self.link = None;
        self.mouse = MouseState::default();
        let Some(program) = find_program(&self.program) else {
            self.phase = Phase::Failed(format!("no such file in PATH: {}", self.program));
            return;
        };
        let injected = settings
            .shell_integration
            .then(|| inject(&program, &self.args, &settings.env))
            .flatten();
        let (args, env) = match &injected {
            Some(injected) => (&injected.args, &injected.env),
            None => (&self.args, &settings.env),
        };
        let launch =
            Launch { program: &program.to_string_lossy(), args, directory: &self.directory, env };
        match Session::start(&launch, settings, notifier) {
            Ok(session) => {
                self.session = Some(session);
                self.phase = Phase::Starting;
            }
            Err(error) => self.phase = Phase::Failed(error),
        }
    }

    /// Takes what the threads reported; `shown` is whether the terminal is on screen.
    pub fn pump(&mut self, shown: bool) -> Pumped {
        let mut pumped = Pumped::default();
        let Some(session) = &self.session else {
            return pumped;
        };
        let mut inbox = lock(&session.inbox);
        if std::mem::take(&mut inbox.output) {
            if self.phase == Phase::Starting {
                self.phase = Phase::Running;
            }
            self.activity |= !shown;
        }
        if std::mem::take(&mut inbox.bell) {
            self.bell |= !shown;
            self.rang = Some(Instant::now());
            pumped.rang = true;
        }
        if let Some(title) = inbox.title.take() {
            self.title = title.filter(|title| !title.trim().is_empty());
        }
        if let Some(directory) = inbox.directory.take() {
            self.directory = directory;
        }
        for (mark, at) in inbox.marks.drain(..) {
            self.commands.apply(mark, at);
        }
        pumped.clipboard = inbox.clipboard.take();
        if let Some(code) = inbox.exit.take() {
            self.phase = Phase::Exited(code);
            pumped.exited = true;
        }
        if shown {
            self.activity = false;
            self.bell = false;
        }
        pumped
    }

    /// Tells a program that asked (DECSET 1004) when the terminal gets or loses the keys.
    pub fn report_focus(&mut self, focused: bool) {
        if self.focused == focused {
            return;
        }
        self.focused = focused;
        if let Some(session) = &self.session
            && session.screen.lock().mode().contains(TermMode::FOCUS_IN_OUT)
        {
            session.write(if focused { "\x1b[I" } else { "\x1b[O" });
        }
    }

    /// What its chip and buffer are called: its name, else the program's title, else
    /// where the shell is.
    pub fn label(&self) -> String {
        self.name
            .clone()
            .or_else(|| self.title.as_deref().map(tidy_title))
            .unwrap_or_else(|| short_directory(&self.directory))
    }

    pub fn buffer_name(&self) -> String {
        format!("*term: {}*", self.label())
    }

    /// The shell's name, e.g. `fish`.
    pub fn shell(&self) -> &str {
        Path::new(&self.program).file_name().and_then(|name| name.to_str()).unwrap_or(&self.program)
    }

    pub fn tab_state(&self) -> TabState {
        match (&self.phase, &self.session) {
            (Phase::Exited(code), _) => TabState::Exited(*code),
            (Phase::Failed(_), _) => TabState::Exited(127),
            (_, Some(session)) if !session.is_idle() => TabState::Running,
            _ => TabState::Idle,
        }
    }
}

/// The shell new terminals run: the configured one, else `$SHELL`, else the user's
/// login shell.
pub fn configured_shell(settings: &TerminalSettings) -> String {
    if !settings.shell.is_empty() {
        return settings.shell.clone();
    }
    env::var("SHELL")
        .ok()
        .filter(|shell| !shell.is_empty())
        .or_else(login_shell)
        .unwrap_or_else(|| FALLBACK_SHELL.to_owned())
}

fn login_shell() -> Option<String> {
    // SAFETY: getpwuid returns a pointer into static storage, read at once.
    unsafe {
        let entry = libc::getpwuid(libc::getuid());
        if entry.is_null() || (*entry).pw_shell.is_null() {
            return None;
        }
        Some(CStr::from_ptr((*entry).pw_shell).to_string_lossy().into_owned())
    }
}

/// `program` as a path: as given when it has a slash, else found on `PATH` or where
/// shells usually are.
pub fn find_program(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let path = expand_home(program);
        return path.is_file().then_some(path);
    }
    let path = env::var("PATH").unwrap_or_default();
    path.split(':')
        .chain(SHELL_DIRECTORIES.iter().copied())
        .map(|directory| Path::new(directory).join(program))
        .find(|candidate| candidate.is_file())
}

/// A shell's title as short as a chip wants it: without the `user@host:` zsh and bash put
/// first, a directory shortened to its last characters, anything else to its first.
fn tidy_title(title: &str) -> String {
    let title = title.trim();
    let title = match title.split_once(':') {
        Some((who, rest)) if who.contains('@') && !who.contains(' ') => rest.trim(),
        _ => title,
    };
    let directory = title.starts_with('~') || title.starts_with('/');
    let title = if directory { short_directory(&expand_home(title)) } else { title.to_owned() };
    let count = title.chars().count();
    if count <= MAX_TITLE {
        return title;
    }
    if directory {
        let tail: String = title.chars().skip(count - (MAX_TITLE - 1)).collect();
        return format!("…{tail}");
    }
    let head: String = title.chars().take(MAX_TITLE - 1).collect();
    format!("{head}…")
}

/// `~/work/shop-api` as `~/w/shop-api`, as fish's `prompt_pwd` shortens it.
pub fn short_directory(directory: &Path) -> String {
    let full = abbreviate_home(directory);
    let mut parts: Vec<String> = full.split('/').map(str::to_owned).collect();
    let last = parts.len().saturating_sub(1);
    for part in &mut parts[..last] {
        if part.chars().count() > 1 && part != "~" {
            let keep = if part.starts_with('.') { 2 } else { 1 };
            *part = part.chars().take(keep).collect();
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens_directories_as_fish_does() {
        assert_eq!(short_directory(Path::new("/usr/local/bin")), "/u/l/bin");
        assert_eq!(short_directory(Path::new("/")), "/");
        assert_eq!(short_directory(Path::new("/a/.config/b")), "/a/.c/b");
    }

    #[test]
    fn tidies_titles_for_chips() {
        assert_eq!(tidy_title("hk@mbp:/usr/local/bin"), "/u/l/bin");
        assert_eq!(tidy_title("cargo watch"), "cargo watch");
        assert_eq!(tidy_title("a: b"), "a: b");
        let long = format!("start {}", "x".repeat(40));
        assert_eq!(tidy_title(&long).chars().count(), MAX_TITLE);
        assert!(tidy_title(&long).starts_with("start x") && tidy_title(&long).ends_with('…'));
        let deep = format!("/a/{}-end", "x".repeat(40));
        assert!(tidy_title(&deep).starts_with('…') && tidy_title(&deep).ends_with("-end"));
    }

    #[test]
    fn finds_shells_by_name_or_path() {
        assert!(find_program("sh").is_some_and(|path| path.ends_with("sh")));
        assert_eq!(find_program("/bin/sh"), Some(PathBuf::from("/bin/sh")));
        assert_eq!(find_program("no-such-shell-anywhere"), None);
    }
}
