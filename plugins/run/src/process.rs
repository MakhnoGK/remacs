//! A launch running on a background thread: its steps one after another, their output
//! through one pipe, as a terminal would interleave it.

use std::io::{self, Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use std::path::Path;
use std::rc::Rc;

use remacs_core::{Editor, Notifier, Tone};

use crate::configuration::Configuration;
use crate::launch::Launch;

const READ_SIZE: usize = 64 * 1024;

/// What a console shows the run of: a launch's processes, or a program another plugin
/// drives, like a debugger's.
pub trait Program {
    fn try_event(&self) -> Option<Event>;
    /// Asks it to end; what it started is killed after `timeout`.
    fn stop(&self, timeout: Duration);
    /// False when the program does not read its input.
    fn write_input(&self, input: &[u8]) -> bool;
    fn close_input(&self);
    /// How it goes, in place of `running`, e.g. `paused`.
    fn state(&self) -> Option<(String, Tone)> {
        None
    }
}

/// Starts a configuration of the project at the root given another way than running it,
/// e.g. under a debugger; called for every run of a session.
pub type Starter =
    Rc<dyn Fn(&mut Editor, &Path, &Configuration, &Launch) -> Result<Box<dyn Program>, String>>;

pub enum Event {
    /// A step starts, as a shell would show its command.
    Started(String),
    Output(Vec<u8>),
    /// The last step ended, or one that failed.
    Exited(Exit),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Code(i32),
    /// Ended by a signal, e.g. when stopped.
    Signal(i32),
    /// Could not be started.
    NotStarted,
}

impl Exit {
    pub fn succeeded(self) -> bool {
        self == Exit::Code(0)
    }

    fn of(status: ExitStatus) -> Self {
        status
            .code()
            .map(Exit::Code)
            .or_else(|| status.signal().map(Exit::Signal))
            .unwrap_or(Exit::NotStarted)
    }
}

/// What the UI thread shares with the process thread.
#[derive(Default)]
struct Control {
    /// The process group of the step running, if one is.
    group: Mutex<Option<i32>>,
    stdin: Mutex<Input>,
    stopping: AtomicBool,
}

/// Where what is typed for the program goes.
enum Input {
    /// No step is running yet: kept for the next one.
    Waiting(Vec<u8>),
    Open(ChildStdin),
    /// Closed by `C-d`, or the steps are over.
    Closed,
}

impl Default for Input {
    fn default() -> Self {
        Input::Waiting(Vec::new())
    }
}

pub struct Process {
    events: Receiver<Event>,
    control: Arc<Control>,
}

impl Process {
    /// Runs `launch`'s steps in turn, each in its own process group so that stopping it
    /// stops what it started too; `notifier` is called whenever there are events.
    pub fn start(launch: Launch, notifier: Notifier) -> Self {
        let (send, events) = mpsc::channel();
        let control = Arc::new(Control::default());
        let shared = Arc::clone(&control);
        thread::Builder::new()
            .name("run-process".into())
            .spawn(move || {
                let exit = run_steps(&launch, &shared, &send, &notifier);
                *lock(&shared.stdin) = Input::Closed;
                let _ = send.send(Event::Exited(exit));
                notifier();
            })
            .expect("spawning a process thread");
        Self { events, control }
    }
}

impl Program for Process {
    fn try_event(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    /// Asks the process group to end, and kills it once `timeout` has passed.
    fn stop(&self, timeout: Duration) {
        self.control.stopping.store(true, Ordering::Relaxed);
        let Some(group) = *lock(&self.control.group) else {
            return;
        };
        signal_group(group, libc::SIGTERM);
        let control = Arc::clone(&self.control);
        thread::Builder::new()
            .name("run-stop".into())
            .spawn(move || {
                thread::sleep(timeout);
                if *lock(&control.group) == Some(group) {
                    signal_group(group, libc::SIGKILL);
                }
            })
            .expect("spawning a stop thread");
    }

    /// Writes `input` to the running step, or keeps it for the step about to start; false
    /// when the program does not read its input.
    fn write_input(&self, input: &[u8]) -> bool {
        let mut stdin = lock(&self.control.stdin);
        let written = match &mut *stdin {
            Input::Waiting(kept) => {
                kept.extend_from_slice(input);
                true
            }
            Input::Open(stdin) => stdin.write_all(input).and_then(|()| stdin.flush()).is_ok(),
            Input::Closed => false,
        };
        if !written {
            *stdin = Input::Closed;
        }
        written
    }

    /// Closes the running step's input, as `C-d` does in a terminal.
    fn close_input(&self) {
        *lock(&self.control.stdin) = Input::Closed;
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn signal_group(group: i32, signal: i32) {
    // SAFETY: `kill` only sends a signal; a group that already ended makes it fail
    // harmlessly with ESRCH.
    unsafe {
        libc::kill(-group, signal);
    }
}

/// The steps' exit: the first that failed, else the last one's.
fn run_steps(
    launch: &Launch,
    control: &Control,
    send: &Sender<Event>,
    notifier: &Notifier,
) -> Exit {
    let shell = |command: &String| {
        let mut shell = Command::new("/bin/sh");
        shell.arg("-c").arg(command);
        (command.clone(), shell)
    };
    let mut main = Command::new(&launch.program);
    main.args(&launch.args);
    let steps = launch
        .before
        .iter()
        .map(shell)
        .chain([(launch.command_line(), main)]);
    let mut exit = Exit::Code(0);
    for (command_line, mut command) in steps {
        if control.stopping.load(Ordering::Relaxed) {
            break;
        }
        let _ = send.send(Event::Started(command_line));
        command
            .current_dir(&launch.cwd)
            .envs(launch.env.iter().map(|(name, value)| (name, value)));
        exit = match run_step(command, control, send, notifier) {
            Ok(exit) => exit,
            Err(error) => {
                let message = format!("Could not start {}: {error}\n", launch.program);
                let _ = send.send(Event::Output(message.into_bytes()));
                Exit::NotStarted
            }
        };
        if !exit.succeeded() {
            break;
        }
    }
    exit
}

fn run_step(
    mut command: Command,
    control: &Control,
    send: &Sender<Event>,
    notifier: &Notifier,
) -> io::Result<Exit> {
    let (mut output, writer) = io::pipe()?;
    command
        .stdin(Stdio::piped())
        .stdout(writer.try_clone()?)
        .stderr(writer)
        .process_group(0);
    let mut child = command.spawn()?;
    // The command holds the pipe's writing ends; the output ends only once they close.
    drop(command);
    let group = i32::try_from(child.id()).map_err(io::Error::other)?;
    *lock(&control.group) = Some(group);
    {
        let mut input = lock(&control.stdin);
        if let (Input::Waiting(kept), Some(mut stdin)) = (&*input, child.stdin.take()) {
            *input = match stdin.write_all(kept) {
                Ok(()) => Input::Open(stdin),
                Err(_) => Input::Closed,
            };
        }
    }
    if control.stopping.load(Ordering::Relaxed) {
        signal_group(group, libc::SIGTERM);
    }
    let mut buffer = vec![0; READ_SIZE];
    loop {
        match output.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                let _ = send.send(Event::Output(buffer[..read].to_vec()));
                notifier();
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    let status = child.wait()?;
    *lock(&control.group) = None;
    *lock(&control.stdin) = Input::default();
    Ok(Exit::of(status))
}
