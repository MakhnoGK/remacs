//! What happens before a debug adapter starts, on a thread of its own: a configuration's
//! commands before launch and, for Cargo, building the program to debug. Their output
//! goes to the run's console.

use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use remacs_core::Notifier;
use remacs_run::{Event, Exit};
use serde_json::Value;

/// The program a debug adapter starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// A path, or a name looked up on `PATH`.
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct Plan {
    /// Shell commands, run in turn in the target's directory.
    pub before: Vec<String>,
    /// Arguments of `cargo build`, whose executable then becomes the target's program.
    pub cargo_build: Option<Vec<String>>,
    pub target: Target,
}

pub enum Prepared {
    Ready(Target),
    Failed(Exit),
}

pub struct Preparation {
    result: Receiver<Prepared>,
    control: Arc<Control>,
}

#[derive(Default)]
struct Control {
    cancelled: AtomicBool,
    /// The process group of the step running.
    group: Mutex<Option<i32>>,
}

/// Tells the console what the steps print.
#[derive(Clone)]
struct Console {
    output: Sender<Event>,
    notifier: Notifier,
}

impl Console {
    fn say(&self, event: Event) {
        let _ = self.output.send(event);
        (self.notifier)();
    }
}

impl Preparation {
    pub fn start(plan: Plan, output: Sender<Event>, notifier: Notifier) -> Self {
        let (send, result) = mpsc::channel();
        let control = Arc::new(Control::default());
        let shared = Arc::clone(&control);
        let console = Console {
            output,
            notifier: notifier.clone(),
        };
        thread::Builder::new()
            .name("debug-prepare".into())
            .spawn(move || {
                let prepared = prepare(plan, &console, &shared);
                let _ = send.send(prepared);
                notifier();
            })
            .expect("spawning a preparing thread");
        Self { result, control }
    }

    pub fn try_result(&self) -> Option<Prepared> {
        self.result.try_recv().ok()
    }

    /// Stops the step running and what it started.
    pub fn cancel(&self) {
        self.control.cancelled.store(true, Ordering::Relaxed);
        if let Some(group) = *lock(&self.control.group) {
            // SAFETY: `kill` only sends a signal; a group already gone makes it fail.
            unsafe {
                libc::kill(-group, libc::SIGTERM);
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn prepare(plan: Plan, console: &Console, control: &Control) -> Prepared {
    let mut target = plan.target;
    for command_line in &plan.before {
        let mut shell = Command::new("/bin/sh");
        shell.arg("-c").arg(command_line);
        console.say(Event::Started(command_line.clone()));
        let exit = run(shell, &target, control, console, |line| {
            console.say(Event::Output(format!("{line}\n").into_bytes()));
        });
        if exit != Exit::Code(0) {
            return Prepared::Failed(exit);
        }
    }
    if let Some(args) = plan.cargo_build {
        let mut cargo = Command::new("cargo");
        cargo
            .arg("build")
            .args(&args)
            .arg("--message-format=json-render-diagnostics");
        console.say(Event::Started(format!("cargo build {}", args.join(" "))));
        let mut executable = None;
        let exit = run(cargo, &target, control, console, |line| {
            if let Some(built) = built_executable(line) {
                executable = Some(built);
            }
        });
        if exit != Exit::Code(0) {
            return Prepared::Failed(exit);
        }
        let Some(executable) = executable else {
            console.say(Event::Output(
                b"cargo built no executable to debug\n".to_vec(),
            ));
            return Prepared::Failed(Exit::NotStarted);
        };
        target.program = executable;
    }
    if control.cancelled.load(Ordering::Relaxed) {
        return Prepared::Failed(Exit::Signal(libc::SIGTERM));
    }
    Prepared::Ready(target)
}

/// The executable a line of `cargo build --message-format=json` says was built.
pub fn built_executable(line: &str) -> Option<String> {
    let message: Value = serde_json::from_str(line).ok()?;
    if message["reason"] != "compiler-artifact" {
        return None;
    }
    message["executable"].as_str().map(str::to_owned)
}

/// Runs `command` in its own process group in the target's directory: stderr goes to the
/// console, each line of stdout to `on_line`.
fn run(
    mut command: Command,
    target: &Target,
    control: &Control,
    console: &Console,
    mut on_line: impl FnMut(&str),
) -> Exit {
    if control.cancelled.load(Ordering::Relaxed) {
        return Exit::Signal(libc::SIGTERM);
    }
    command
        .current_dir(&target.cwd)
        .envs(target.env.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let program = command.get_program().to_string_lossy().into_owned();
            let message = format!("Could not start {program}: {error}\n");
            console.say(Event::Output(message.into_bytes()));
            return Exit::NotStarted;
        }
    };
    *lock(&control.group) = i32::try_from(child.id()).ok();
    let copying = child
        .stderr
        .take()
        .map(|stderr| copy_to(stderr, console.clone()));
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => on_line(&line),
                Err(_) => break,
            }
        }
    }
    if let Some(copying) = copying {
        let _ = copying.join();
    }
    let status = child.wait();
    *lock(&control.group) = None;
    match status {
        Ok(status) => status
            .code()
            .map(Exit::Code)
            .or_else(|| status.signal().map(Exit::Signal))
            .unwrap_or(Exit::NotStarted),
        Err(_) => Exit::NotStarted,
    }
}

fn copy_to(mut stream: impl Read + Send + 'static, console: Console) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = vec![0; 16 * 1024];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => return,
                Ok(read) => console.say(Event::Output(buffer[..read].to_vec())),
            }
        }
    })
}
