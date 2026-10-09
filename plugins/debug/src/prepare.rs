//! What happens before a debug adapter starts, on a thread of its own: a configuration's
//! commands before launch and, for Cargo, building the program to debug. Their output
//! goes to the run's console.

use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
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
    /// Builds the program to debug, which then becomes the target's.
    pub cargo_build: Option<CargoBuild>,
    pub target: Target,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CargoBuild {
    /// Builds test binaries with `cargo test --no-run` rather than with `cargo build`.
    pub tests: bool,
    /// The package and target, then cargo's own arguments.
    pub args: Vec<String>,
}

pub enum Prepared {
    Ready(Target),
    /// Several test binaries were built; the user picks one, by its label.
    Choose(Vec<(String, Target)>),
    Failed(Exit),
}

/// An executable cargo says it built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Built {
    pub executable: String,
    /// The target and its kind, e.g. `api (lib)`.
    pub label: String,
    pub package_dir: PathBuf,
    /// Built to run the target's tests.
    pub test: bool,
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
    let target = plan.target;
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
    let Some(build) = plan.cargo_build else {
        return ready(target, control);
    };
    let mut cargo = Command::new("cargo");
    let command = if build.tests {
        cargo.args(["test", "--no-run"]);
        "cargo test --no-run"
    } else {
        cargo.arg("build");
        "cargo build"
    };
    cargo
        .args(&build.args)
        .arg("--message-format=json-render-diagnostics");
    console.say(Event::Started(format!(
        "{command} {}",
        build.args.join(" ")
    )));
    let mut executables = Vec::new();
    let exit = run(cargo, &target, control, console, |line| {
        executables.extend(built_executable(line).filter(|built| built.test == build.tests));
    });
    if exit != Exit::Code(0) {
        return Prepared::Failed(exit);
    }
    let mut choices: Vec<(String, Target)> = executables
        .into_iter()
        .map(|built| {
            let target = Target {
                program: built.executable,
                // Where `cargo test` runs them.
                cwd: if build.tests {
                    built.package_dir
                } else {
                    target.cwd.clone()
                },
                ..target.clone()
            };
            (built.label, target)
        })
        .collect();
    match choices.len() {
        0 => {
            let message = format!("{command} built no executable to debug\n");
            console.say(Event::Output(message.into_bytes()));
            Prepared::Failed(Exit::NotStarted)
        }
        // `cargo build` names the executable asked for last.
        _ if !build.tests => ready(choices.pop().expect("one at least").1, control),
        1 => ready(choices.remove(0).1, control),
        _ => Prepared::Choose(choices),
    }
}

fn ready(target: Target, control: &Control) -> Prepared {
    if control.cancelled.load(Ordering::Relaxed) {
        return Prepared::Failed(Exit::Signal(libc::SIGTERM));
    }
    Prepared::Ready(target)
}

/// The executable a line of `cargo build --message-format=json` says was built.
pub fn built_executable(line: &str) -> Option<Built> {
    let message: Value = serde_json::from_str(line).ok()?;
    if message["reason"] != "compiler-artifact" {
        return None;
    }
    let target = &message["target"];
    let name = target["name"].as_str().unwrap_or_default();
    let kind = target["kind"][0].as_str().unwrap_or_default();
    let package_dir = Path::new(message["manifest_path"].as_str().unwrap_or_default())
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    Some(Built {
        executable: message["executable"].as_str()?.to_owned(),
        label: format!("{name} ({kind})"),
        package_dir,
        test: message["profile"]["test"].as_bool().unwrap_or(false),
    })
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
