//! One terminal: a shell on a pseudoterminal, its output parsed into a screen with
//! scrollback on a background thread, what is typed written to it on another.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{self, Term};
use alacritty_terminal::tty::{self, Pty, Shell};
use alacritty_terminal::vte::ansi::{CursorShape, CursorStyle, Processor, Rgb};
use remacs_core::{Notifier, ScreenSize};

use crate::scan::{END_LINK, Mark, Report, Scanner, prompt_link};
use crate::settings::TerminalSettings;

const READ_SIZE: usize = 64 * 1024;
/// Until the first layout tells the real size.
const INITIAL_SIZE: ScreenSize = ScreenSize { columns: 80, rows: 24 };
/// What ends a word for `M-f`, `M-b` and a double click: punctuation, but not what
/// paths and flags are made of, so that one is taken whole.
const WORD_SEPARATORS: &str = ",│`|:\"' ()[]{}<>\t;=!@#$%^&*+\\?";
/// Answered to programs that ask for the window's pixel size.
const CELL_WIDTH: u16 = 8;
const CELL_HEIGHT: u16 = 22;

pub type Screen = Arc<FairMutex<Term<Listener>>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Nothing printed yet.
    Starting,
    Running,
    Exited(i32),
    /// The shell could not be started.
    Failed(String),
}

/// What the threads have for the UI since it last looked.
#[derive(Default)]
pub struct Inbox {
    /// Something was printed.
    pub output: bool,
    pub bell: bool,
    /// Set by the program; `Some(None)` resets it.
    pub title: Option<Option<String>>,
    /// The shell's working directory, as it reports it.
    pub directory: Option<PathBuf>,
    pub exit: Option<i32>,
    /// Asked to be put on the clipboard.
    pub clipboard: Option<String>,
    /// Where commands' prompts start and how they end, in order, and when.
    pub marks: Vec<(Mark, SystemTime)>,
}

/// Hears the emulator; it runs on the reading thread.
pub struct Listener {
    inbox: Arc<Mutex<Inbox>>,
    input: Sender<Vec<u8>>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Title(title) => lock(&self.inbox).title = Some(Some(title)),
            Event::ResetTitle => lock(&self.inbox).title = Some(None),
            Event::Bell => lock(&self.inbox).bell = true,
            Event::ClipboardStore(_, text) => lock(&self.inbox).clipboard = Some(text),
            Event::PtyWrite(text) => {
                let _ = self.input.send(text.into_bytes());
            }
            Event::ColorRequest(index, format) => {
                let _ = self.input.send(format(palette(index)).into_bytes());
            }
            Event::TextAreaSizeRequest(format) => {
                let size = WindowSize {
                    num_lines: 0,
                    num_cols: 0,
                    cell_width: CELL_WIDTH,
                    cell_height: CELL_HEIGHT,
                };
                let _ = self.input.send(format(size).into_bytes());
            }
            _ => {}
        }
    }
}

/// The color a program asking for one by index gets: the theme's, the text's for the
/// foreground and the theme's background for the glass.
fn palette(index: usize) -> Rgb {
    let rgb = match index {
        0..16 => remacs_theme::color::ansi(index),
        256 | 258 => remacs_theme::color::fg_body(),
        _ => remacs_theme::color::bg_main(),
    };
    Rgb { r: (rgb >> 16) as u8, g: (rgb >> 8) as u8, b: rgb as u8 }
}

/// A running terminal; dropping it hangs its programs up and kills what is left.
pub struct Session {
    pub screen: Screen,
    pub inbox: Arc<Mutex<Inbox>>,
    input: Sender<Vec<u8>>,
    pty: File,
    /// The shell, leader of its own session.
    pid: i32,
    /// Set while the screen is drawn, which fits it to the room it has.
    size: Cell<ScreenSize>,
    kill_timeout: Duration,
}

/// How a shell is started.
pub struct Launch<'a> {
    pub program: &'a str,
    pub args: &'a [String],
    pub directory: &'a Path,
    pub env: &'a BTreeMap<String, String>,
}

impl Session {
    pub fn start(
        launch: &Launch,
        settings: &TerminalSettings,
        notifier: Notifier,
    ) -> Result<Self, String> {
        let options = tty::Options {
            shell: Some(Shell::new(launch.program.to_owned(), launch.args.to_vec())),
            working_directory: Some(launch.directory.to_path_buf()),
            drain_on_exit: true,
            env: launch.env.iter().map(|(name, value)| (name.clone(), value.clone())).collect(),
        };
        let pty =
            tty::new(&options, window_size(INITIAL_SIZE), 0).map_err(|error| error.to_string())?;
        let (input, typed) = mpsc::channel::<Vec<u8>>();
        let inbox = Arc::new(Mutex::new(Inbox::default()));
        let listener = Listener { inbox: Arc::clone(&inbox), input: input.clone() };
        let config = term::Config {
            scrolling_history: settings.scrollback_lines,
            semantic_escape_chars: WORD_SEPARATORS.to_owned(),
            default_cursor_style: CursorStyle {
                shape: match settings.cursor.shape() {
                    remacs_core::CursorShape::Bar => CursorShape::Beam,
                    remacs_core::CursorShape::Underline => CursorShape::Underline,
                    _ => CursorShape::Block,
                },
                blinking: false,
            },
            kitty_keyboard: settings.kitty_keyboard,
            ..term::Config::default()
        };
        let size = TermSize::new(INITIAL_SIZE.columns, INITIAL_SIZE.rows);
        let screen = Arc::new(FairMutex::new(Term::new(config, &size, listener)));
        let file = pty.file().try_clone().map_err(|error| error.to_string())?;
        set_blocking(file.as_raw_fd());
        let pid = pty.child().id() as i32;
        let writer = file.try_clone().map_err(|error| error.to_string())?;
        thread::Builder::new()
            .name("terminal-write".into())
            .spawn(move || write_input(writer, typed))
            .map_err(|error| error.to_string())?;
        let reader =
            Reader { pty, screen: Arc::clone(&screen), inbox: Arc::clone(&inbox), notifier };
        thread::Builder::new()
            .name("terminal-read".into())
            .spawn(move || reader.run())
            .map_err(|error| error.to_string())?;
        Ok(Self {
            screen,
            inbox,
            input,
            pty: file,
            pid,
            size: Cell::new(INITIAL_SIZE),
            kill_timeout: Duration::from_millis(settings.kill_timeout),
        })
    }

    pub fn write(&self, bytes: impl Into<Vec<u8>>) {
        let _ = self.input.send(bytes.into());
    }

    /// Fits the screen and tells the programs, as a window resized would.
    pub fn resize(&self, size: ScreenSize) {
        if size == self.size.get() || size.columns == 0 || size.rows == 0 {
            return;
        }
        self.size.set(size);
        self.screen.lock().resize(TermSize::new(size.columns, size.rows));
        let winsize = libc::winsize {
            ws_row: size.rows as u16,
            ws_col: size.columns as u16,
            ws_xpixel: size.columns as u16 * CELL_WIDTH,
            ws_ypixel: size.rows as u16 * CELL_HEIGHT,
        };
        // SAFETY: the descriptor is the pseudoterminal's, open while `self` is.
        unsafe { libc::ioctl(self.pty.as_raw_fd(), libc::TIOCSWINSZ, &winsize) };
    }

    pub fn size(&self) -> ScreenSize {
        self.size.get()
    }

    /// Whether the program in the foreground is the shell itself, waiting for a command.
    pub fn is_idle(&self) -> bool {
        // SAFETY: as in `resize`.
        let foreground = unsafe { libc::tcgetpgrp(self.pty.as_raw_fd()) };
        foreground <= 0 || foreground == self.pid
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: as in `resize`.
        let foreground = unsafe { libc::tcgetpgrp(self.pty.as_raw_fd()) };
        let groups = [self.pid, foreground];
        signal(&groups, libc::SIGHUP);
        let timeout = self.kill_timeout;
        let _ = thread::Builder::new().name("terminal-kill".into()).spawn(move || {
            thread::sleep(timeout);
            signal(&groups, libc::SIGKILL);
        });
    }
}

fn signal(groups: &[i32], signal: i32) {
    for &group in groups.iter().filter(|&&group| group > 0) {
        // SAFETY: signalling a process group has no memory effects.
        unsafe { libc::kill(-group, signal) };
    }
}

fn window_size(size: ScreenSize) -> WindowSize {
    WindowSize {
        num_lines: size.rows as u16,
        num_cols: size.columns as u16,
        cell_width: CELL_WIDTH,
        cell_height: CELL_HEIGHT,
    }
}

/// alacritty leaves the pseudoterminal non-blocking for its own event loop; these
/// threads block instead.
fn set_blocking(fd: RawFd) {
    // SAFETY: the descriptor is open; only its status flags change.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
        libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK);
    }
}

fn write_input(mut pty: File, typed: mpsc::Receiver<Vec<u8>>) {
    while let Ok(bytes) = typed.recv() {
        if pty.write_all(&bytes).is_err() {
            break;
        }
    }
}

struct Reader {
    pty: Pty,
    screen: Screen,
    inbox: Arc<Mutex<Inbox>>,
    notifier: Notifier,
}

impl Reader {
    fn run(self) {
        let mut parser: Processor = Processor::new();
        let mut scanner = Scanner::default();
        let mut prompts = 0;
        let mut buffer = vec![0; READ_SIZE];
        let mut file = match self.pty.file().try_clone() {
            Ok(file) => file,
            Err(_) => return,
        };
        loop {
            if !wait_readable(file.as_raw_fd(), parser.sync_timeout().sync_timeout()) {
                parser.stop_sync(&mut *self.screen.lock());
                (self.notifier)();
                continue;
            }
            let read = match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                // EIO once every program holding the terminal has ended.
                Err(_) => break,
            };
            {
                let mut screen = self.screen.lock();
                for piece in scanner.split(&buffer[..read]) {
                    parser.advance(&mut *screen, piece.bytes);
                    if let Some(report) = piece.report {
                        self.take_report(report, &mut parser, &mut screen, &mut prompts);
                    }
                }
            }
            lock(&self.inbox).output = true;
            (self.notifier)();
        }
        let code = exit_code(self.pty.child().id() as i32);
        lock(&self.inbox).exit = Some(code);
        (self.notifier)();
    }

    /// A prompt's cells are tagged with a hyperlink of its own until its command starts,
    /// so its mark stays with its row however the screen scrolls or rewraps.
    fn take_report(
        &self,
        report: Report,
        parser: &mut Processor,
        screen: &mut Term<Listener>,
        prompts: &mut u64,
    ) {
        let mark = match report {
            Report::Directory(directory) => {
                lock(&self.inbox).directory = Some(directory);
                return;
            }
            Report::Prompt => {
                *prompts += 1;
                parser.advance(screen, prompt_link(*prompts).as_bytes());
                Mark::Prompt(*prompts)
            }
            Report::Input => {
                parser.advance(screen, END_LINK);
                return;
            }
            Report::Started => {
                parser.advance(screen, END_LINK);
                Mark::Started
            }
            Report::Finished(code) => {
                parser.advance(screen, END_LINK);
                Mark::Finished(code)
            }
        };
        lock(&self.inbox).marks.push((mark, SystemTime::now()));
    }
}

/// Waits until the terminal has output, or the synchronized update's deadline passes;
/// false for the deadline.
fn wait_readable(fd: RawFd, deadline: Option<Instant>) -> bool {
    let Some(deadline) = deadline else {
        return true;
    };
    let timeout = deadline.saturating_duration_since(Instant::now());
    let mut poll = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    // SAFETY: one valid pollfd for the duration of the call.
    let ready = unsafe { libc::poll(&mut poll, 1, timeout.as_millis() as i32) };
    ready != 0
}

/// The shell's exit code, read without reaping it, which alacritty's `Pty` does when
/// dropped; a signal counts as 128 plus its number, as shells report it.
fn exit_code(pid: i32) -> i32 {
    // SAFETY: `siginfo_t` is plain data, valid zeroed.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is a local the call writes; WNOWAIT leaves the child to be reaped.
    let waited = unsafe {
        libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOWAIT)
    };
    if waited != 0 {
        return 0;
    }
    // SAFETY: `waitid` succeeded, so `info` describes the child.
    let status = unsafe { info.si_status() };
    match info.si_code {
        libc::CLD_EXITED => status,
        _ => 128 + status,
    }
}

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
