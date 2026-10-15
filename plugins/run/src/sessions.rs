//! Configurations run so far, each with its console; one session per configuration,
//! reused by every run of it.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use remacs_core::{CommandResult, Editor, Tone};

use crate::configuration::Configuration;
use crate::console::{self, Console, colored, dim};
use crate::launch::launch;
use crate::process::{Event, Exit, Process, Program, Starter};
use crate::settings::RunSettings;

/// While a program runs, how often the status pill's clock is redrawn.
const CLOCK_INTERVAL: Duration = Duration::from_secs(1);
const RED: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Running,
    /// Asked to stop, not yet ended.
    Stopping,
    Exited(Exit),
    /// Ended by Stop.
    Stopped,
}

pub struct Session {
    pub id: u64,
    pub root: PathBuf,
    pub configuration: Configuration,
    pub state: State,
    pub started: Instant,
    pub finished: Option<Instant>,
    process: Option<Box<dyn Program>>,
    /// Starts it other than by running it, e.g. under a debugger.
    pub starter: Option<Starter>,
    /// Runs again once the process it stopped has ended.
    rerun: bool,
    /// The project's `compile` session, whatever command it runs, rather than a run of a
    /// configuration.
    pub compile: bool,
    /// The name its console is made with.
    console_name: String,
}

impl Session {
    pub fn is_running(&self) -> bool {
        matches!(self.state, State::Running | State::Stopping)
    }

    /// What its program says of how it goes, in place of `running`.
    pub fn program_state(&self) -> Option<(String, Tone)> {
        self.process.as_ref()?.state()
    }

    /// How long it ran, or has run so far.
    pub fn elapsed(&self) -> Duration {
        self.finished.unwrap_or_else(Instant::now) - self.started
    }

    /// Its console buffer's name.
    pub fn buffer_name(editor: &Editor, id: u64) -> Option<String> {
        editor
            .buffers()
            .iter()
            .find(|buffer| {
                buffer
                    .local::<Console>()
                    .is_some_and(|console| console.session == id)
            })
            .map(|buffer| buffer.name().to_owned())
    }
}

#[derive(Default)]
pub struct Sessions {
    pub list: Vec<Session>,
    next_id: u64,
    pub settings: RunSettings,
}

pub fn sessions(editor: &mut Editor) -> &mut Sessions {
    editor.plugin_state()
}

/// The session of the configuration named `name` in the project at `root`.
pub fn find(editor: &Editor, root: &Path, name: &str) -> Option<u64> {
    editor
        .find_plugin_state::<Sessions>()?
        .list
        .iter()
        .find(|session| {
            !session.compile && session.root == root && session.configuration.name == name
        })
        .map(|session| session.id)
}

/// The `compile` session of the project at `root`.
pub fn find_compile(editor: &Editor, root: &Path) -> Option<u64> {
    editor
        .find_plugin_state::<Sessions>()?
        .list
        .iter()
        .find(|session| session.compile && session.root == root)
        .map(|session| session.id)
}

/// The session started last.
pub fn latest(editor: &Editor) -> Option<u64> {
    editor
        .find_plugin_state::<Sessions>()?
        .list
        .iter()
        .max_by_key(|session| session.started)
        .map(|session| session.id)
}

pub fn any_running(editor: &Editor) -> bool {
    editor
        .find_plugin_state::<Sessions>()
        .is_some_and(|sessions| sessions.list.iter().any(Session::is_running))
}

pub fn get(editor: &Editor, id: u64) -> Option<&Session> {
    editor
        .find_plugin_state::<Sessions>()?
        .list
        .iter()
        .find(|session| session.id == id)
}

fn get_mut(editor: &mut Editor, id: u64) -> Option<&mut Session> {
    sessions(editor)
        .list
        .iter_mut()
        .find(|session| session.id == id)
}

/// The session whose console is the window buffer.
pub fn of_window(editor: &Editor) -> Option<u64> {
    editor
        .window_buffer()
        .local::<Console>()
        .map(|console| console.session)
}

/// Runs `configuration` of the project at `root` in its console, shown in the window,
/// or starts it with `starter`; a run still going is stopped first.
pub fn run(
    editor: &mut Editor,
    root: &Path,
    configuration: Configuration,
    starter: Option<Starter>,
) -> CommandResult {
    run_in(editor, root, configuration, starter, false)
}

/// Runs a shell command line as the `compile` of the project at `root`, in its console
/// `*compile: <project>*`; a compile still going is stopped first.
pub fn compile(editor: &mut Editor, root: &Path, configuration: Configuration) -> CommandResult {
    run_in(editor, root, configuration, None, true)
}

/// Runs the session again as it was last started, or with `starter` when given.
pub fn rerun(editor: &mut Editor, id: u64, starter: Option<Starter>) -> CommandResult {
    let session = get_mut(editor, id).ok_or("No such run")?;
    let Some(starter) = starter else {
        return run_session(editor, id);
    };
    let earlier = session.starter.replace(starter);
    let result = run_session(editor, id);
    if result.is_err()
        && let Some(session) = get_mut(editor, id)
    {
        session.starter = earlier;
    }
    result
}

/// Runs `configuration` in its session, the project's `compile` one if `compile`, made
/// first if there is none.
fn run_in(
    editor: &mut Editor,
    root: &Path,
    configuration: Configuration,
    starter: Option<Starter>,
    compile: bool,
) -> CommandResult {
    let settings = sessions(editor).settings.clone();
    launch(&configuration, root, &settings)?;
    let found = if compile {
        find_compile(editor, root)
    } else {
        find(editor, root, &configuration.name)
    };
    let id = match found {
        Some(id) => {
            let session = get_mut(editor, id).expect("found");
            session.configuration = configuration;
            session.starter = starter;
            id
        }
        None => {
            let console_name = if compile {
                format!("*compile: {}*", directory_name(root))
            } else {
                format!("*run: {}*", configuration.name)
            };
            let sessions = sessions(editor);
            sessions.next_id += 1;
            let id = sessions.next_id;
            sessions.list.push(Session {
                id,
                root: root.to_path_buf(),
                configuration,
                state: State::Exited(Exit::Code(0)),
                started: Instant::now(),
                finished: None,
                process: None,
                starter,
                rerun: false,
                compile,
                console_name,
            });
            id
        }
    };
    run_session(editor, id)
}

fn directory_name(root: &Path) -> String {
    root.file_name().map_or_else(
        || root.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Shows the session's console and starts it, once the run going on, if any, has ended.
fn run_session(editor: &mut Editor, id: u64) -> CommandResult {
    show(editor, id);
    let session = get_mut(editor, id).expect("just made");
    if session.is_running() {
        session.rerun = true;
        stop(editor, id);
        return Ok(());
    }
    start(editor, id)
}

/// Starts the session's configuration afresh in its emptied console.
fn start(editor: &mut Editor, id: u64) -> CommandResult {
    let settings = sessions(editor).settings.clone();
    let notifier = editor.notifier();
    let session = get(editor, id).ok_or("No such run")?;
    let launch = launch(&session.configuration, &session.root, &settings)?;
    let process: Box<dyn Program> = match session.starter.clone() {
        Some(starter) => {
            let (root, configuration) = (session.root.clone(), session.configuration.clone());
            starter(editor, &root, &configuration, &launch)?
        }
        None => Box::new(Process::start(launch.clone(), notifier)),
    };
    let session = get_mut(editor, id).ok_or("No such run")?;
    session.process = Some(process);
    session.state = State::Running;
    session.started = Instant::now();
    session.finished = None;
    session.rerun = false;
    let name = Session::buffer_name(editor, id);
    if let Some(buffer) = name.and_then(|name| editor.buffer_named_mut(&name)) {
        console::clear(buffer);
        if let Some(console) = buffer.local_mut::<Console>() {
            console.directories[0] = launch.cwd;
        }
    }
    editor.refresh_after(CLOCK_INTERVAL);
    Ok(())
}

/// Shows the session's console, made first if it has none.
pub fn show(editor: &mut Editor, id: u64) {
    if let Some(name) = Session::buffer_name(editor, id) {
        editor.switch_to_buffer(&name);
        return;
    }
    let Some(session) = get(editor, id) else {
        return;
    };
    let settings = &editor
        .find_plugin_state::<Sessions>()
        .expect("has a session")
        .settings;
    let cwd = launch(&session.configuration, &session.root, settings)
        .map_or_else(|_| session.root.clone(), |launch| launch.cwd);
    let buffer = console::new_buffer(session.console_name.clone(), id, &cwd, &session.root);
    editor.add_buffer(buffer);
}

/// Asks the session's process to end; it is killed if it does not.
pub fn stop(editor: &mut Editor, id: u64) {
    let timeout = sessions(editor).settings.stop_timeout();
    let Some(session) = get_mut(editor, id) else {
        return;
    };
    if let Some(process) = &session.process {
        process.stop(timeout);
        session.state = State::Stopping;
    }
}

/// Sends a line to the session's process.
pub fn send_input(editor: &mut Editor, id: u64, input: &str) -> CommandResult {
    let process = get(editor, id)
        .and_then(|session| session.process.as_ref())
        .ok_or("The program is not running")?;
    if !process.write_input(input.as_bytes()) {
        return Err("The program does not read its input".into());
    }
    // Not to a terminal, the program does not echo what it reads.
    let max_lines = sessions(editor).settings.console_lines;
    let name = Session::buffer_name(editor, id);
    if let Some(buffer) = name.and_then(|name| editor.buffer_named_mut(&name)) {
        console::append_output(buffer, input.as_bytes(), max_lines);
    }
    Ok(())
}

pub fn close_input(editor: &mut Editor, id: u64) -> CommandResult {
    let process = get(editor, id)
        .and_then(|session| session.process.as_ref())
        .ok_or("The program is not running")?;
    process.close_input();
    Ok(())
}

/// Takes what the processes printed into their consoles and notes those that ended; a
/// process whose console was killed is stopped.
pub fn pick_up(editor: &mut Editor) {
    let ids: Vec<u64> = sessions(editor)
        .list
        .iter()
        .map(|session| session.id)
        .collect();
    let max_lines = sessions(editor).settings.console_lines;
    for id in ids {
        let Some(name) = Session::buffer_name(editor, id) else {
            stop(editor, id);
            sessions(editor).list.retain(|session| session.id != id);
            continue;
        };
        let mut events = Vec::new();
        if let Some(process) = get(editor, id).and_then(|session| session.process.as_ref()) {
            while let Some(event) = process.try_event() {
                events.push(event);
            }
        }
        for event in events {
            let buffer = editor.buffer_named_mut(&name).expect("found by name");
            match event {
                Event::Started(command_line) => console::append_note(buffer, &command_line, dim()),
                Event::Output(bytes) => console::append_output(buffer, &bytes, max_lines),
                Event::Exited(exit) => exited(editor, id, &name, exit),
            }
        }
    }
    if sessions(editor).list.iter().any(Session::is_running) {
        editor.refresh_after(CLOCK_INTERVAL);
    }
}

fn exited(editor: &mut Editor, id: u64, name: &str, exit: Exit) {
    let session = get_mut(editor, id).expect("exited a session");
    session.process = None;
    session.finished = Some(Instant::now());
    let stopped = session.state == State::Stopping;
    session.state = if stopped {
        State::Stopped
    } else {
        State::Exited(exit)
    };
    let rerun = session.rerun;
    if session.compile && !rerun {
        report_compile(editor, stopped, exit);
    }
    let buffer = editor.buffer_named_mut(name).expect("exited in a console");
    match (stopped, exit) {
        (true, _) => console::append_note(buffer, "Process stopped", dim()),
        (false, Exit::Code(0)) => console::append_note(buffer, "Process exited with code 0", dim()),
        (false, Exit::Code(code)) => console::append_note(
            buffer,
            &format!("Process exited with code {code}"),
            colored(RED),
        ),
        (false, Exit::Signal(signal)) => console::append_note(
            buffer,
            &format!("Process ended by signal {signal}"),
            colored(RED),
        ),
        (false, Exit::NotStarted) => {}
    }
    if rerun && let Err(error) = start(editor, id) {
        editor.set_error(error.0);
    }
}

/// Says in the echo area how a compile ended, as Emacs does, its console perhaps hidden.
fn report_compile(editor: &mut Editor, stopped: bool, exit: Exit) {
    match (stopped, exit) {
        (true, _) => editor.set_message("Compilation stopped"),
        (false, Exit::Code(0)) => editor.set_message("Compilation finished"),
        (false, Exit::Code(code)) => {
            editor.set_error(format!("Compilation exited abnormally with code {code}"));
        }
        (false, Exit::Signal(signal)) => {
            editor.set_error(format!("Compilation ended by signal {signal}"));
        }
        (false, Exit::NotStarted) => editor.set_error("Compilation could not start"),
    }
}
