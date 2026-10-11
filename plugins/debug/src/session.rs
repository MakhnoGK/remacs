//! Debug sessions: one per configuration debugged, each talking to its adapter over one
//! connection, and over more when the adapter asks for them (js-debug starts a session
//! of its own for each process it debugs).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{self, Sender};
use std::time::Instant;

use remacs_core::{Candidate, CommandResult, Editor, Prompt, Tone, filter_candidates};
use remacs_run::{Configuration, Event, Exit, Launch, Program};
use serde_json::{Value, json};

use crate::adapter::{Adapter, plan};
use crate::breakpoints::Breakpoints;
use crate::dap::{Connection, Incoming, free_port};
use crate::inspect::{self, Inspected, View};
use crate::prepare::{Preparation, Prepared, Target};
use crate::program::{DebugProgram, Shared};
use crate::settings::DebugSettings;

type Handler = Box<dyn FnOnce(&mut Editor, Address, Result<Value, String>)>;

/// A connection of a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Address {
    pub session: u64,
    pub link: usize,
}

#[derive(Default)]
pub struct Debugger {
    pub sessions: Vec<Session>,
    pub breakpoints: Breakpoints,
    pub view: View,
    pub settings: DebugSettings,
    next_id: u64,
    /// The connection that stopped last, which stepping acts on.
    pub current: Option<Address>,
}

pub fn debugger(editor: &mut Editor) -> &mut Debugger {
    editor.plugin_state()
}

enum Phase {
    /// Running the commands before launch, or building.
    Preparing(Preparation),
    /// Waiting for the user to pick one of several test binaries.
    Choosing,
    /// The adapter is being downloaded or started.
    Starting,
    Debugging,
    /// Asked to stop; ends at the deadline if the adapter does not end it first.
    Stopping(Instant),
}

pub struct Session {
    pub id: u64,
    pub name: String,
    /// Of the project debugged.
    root: PathBuf,
    rust: bool,
    adapter: Adapter,
    output: Sender<Event>,
    shared: Rc<Shared>,
    phase: Phase,
    /// The first to the adapter, then those it asked for.
    pub links: Vec<Link>,
    /// Where js-debug listens, for the sessions it asks for.
    port: Option<u16>,
    exit_code: Option<i32>,
    /// What the adapter last logged.
    last_log: Option<String>,
}

pub struct Link {
    connection: Connection,
    pending: HashMap<i64, Handler>,
    /// `launch` or `attach`, with its arguments, sent once initialized.
    request: (String, Value),
    pub stopped: Option<Stop>,
    pub frames: Vec<Frame>,
    pub frame: usize,
    /// Threads, variables and watches while stopped.
    pub inspected: Inspected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stop {
    pub thread: i64,
    /// E.g. `breakpoint`, `step`, `exception`.
    pub reason: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub id: i64,
    pub name: String,
    pub path: Option<PathBuf>,
    /// From 1, as the adapter counts.
    pub line: usize,
    pub column: usize,
    /// Of a library, or the runtime: not the program's own code.
    pub subtle: bool,
}

impl Session {
    pub fn link(&self, index: usize) -> Option<&Link> {
        self.links.get(index)
    }

    /// The adapter's name.
    pub fn adapter(&self) -> &'static str {
        self.adapter.tool()
    }

    /// The adapter runs the program, rather than it being prepared or ended.
    pub fn is_debugging(&self) -> bool {
        !self.links.is_empty() && matches!(self.phase, Phase::Debugging | Phase::Stopping(_))
    }

    fn link_mut(&mut self, index: usize) -> Option<&mut Link> {
        self.links.get_mut(index)
    }

    fn say(&self, event: Event) {
        let _ = self.output.send(event);
    }

    /// Text in the console, as the program would print it.
    pub fn print(&self, text: &str) {
        self.say(Event::Output(text.as_bytes().to_vec()));
    }

    /// A line in the console, dimmed: the debugger's own words.
    pub fn note(&self, text: &str) {
        let text = text.trim_end_matches('\n');
        self.say(Event::Output(
            format!("\x1b[2m{text}\x1b[0m\n").into_bytes(),
        ));
    }
}

fn find_session(editor: &mut Editor, id: u64) -> Option<&mut Session> {
    debugger(editor)
        .sessions
        .iter_mut()
        .find(|session| session.id == id)
}

pub fn find_link(editor: &mut Editor, address: Address) -> Option<&mut Link> {
    find_session(editor, address.session)?.link_mut(address.link)
}

/// Starts debugging `configuration`, run as `launch`: the run console's program.
pub fn start(
    editor: &mut Editor,
    root: &Path,
    configuration: &Configuration,
    launch: &Launch,
) -> Result<Box<dyn Program>, String> {
    let (adapter, mut plan) = plan(configuration, launch)?;
    // Under the run's own variables, which come later and so win.
    let env = debugger(editor).settings.env.clone().into_iter();
    plan.target.env = env.chain(plan.target.env).collect();
    let rust = plan.cargo_build.is_some();
    let notifier = editor.notifier();
    let (output, events) = mpsc::channel();
    let shared = Rc::new(Shared::default());
    shared.set_state("starting", Tone::Faint);
    let preparation = Preparation::start(plan, output.clone(), notifier.clone());
    let debugger = debugger(editor);
    debugger.next_id += 1;
    debugger.sessions.push(Session {
        id: debugger.next_id,
        name: configuration.name.clone(),
        root: root.to_path_buf(),
        rust,
        adapter,
        output,
        shared: Rc::clone(&shared),
        phase: Phase::Preparing(preparation),
        links: Vec::new(),
        port: None,
        exit_code: None,
        last_log: None,
    });
    Ok(Box::new(DebugProgram {
        events,
        shared,
        notifier,
    }))
}

/// Takes what preparations and adapters have to say; runs after every command.
pub fn update(editor: &mut Editor) {
    let ids: Vec<u64> = debugger(editor)
        .sessions
        .iter()
        .map(|session| session.id)
        .collect();
    for id in ids {
        stop_if_asked(editor, id);
        prepared(editor, id);
        receive(editor, id);
        if let Some(Phase::Stopping(deadline)) =
            find_session(editor, id).map(|session| &session.phase)
            && Instant::now() >= *deadline
        {
            finish(editor, id, None);
        }
    }
}

fn stop_if_asked(editor: &mut Editor, id: u64) {
    let Some(session) = find_session(editor, id) else {
        return;
    };
    let Some(timeout) = session.shared.stop.take() else {
        return;
    };
    match &session.phase {
        Phase::Preparing(preparation) => preparation.cancel(),
        Phase::Choosing | Phase::Starting => finish(editor, id, Some(Exit::Signal(libc::SIGTERM))),
        Phase::Stopping(_) => {}
        Phase::Debugging => {
            session.phase = Phase::Stopping(Instant::now() + timeout);
            editor.refresh_after(timeout);
            let address = Address {
                session: id,
                link: 0,
            };
            request(
                editor,
                address,
                "disconnect",
                json!({ "terminateDebuggee": true }),
                |editor, address, _| finish(editor, address.session, None),
            );
        }
    }
}

fn prepared(editor: &mut Editor, id: u64) {
    let Some(session) = find_session(editor, id) else {
        return;
    };
    let Phase::Preparing(preparation) = &session.phase else {
        return;
    };
    match preparation.try_result() {
        None => {}
        Some(Prepared::Failed(exit)) => finish(editor, id, Some(exit)),
        Some(Prepared::Ready(target)) => start_adapter(editor, id, target),
        Some(Prepared::Choose(choices)) => {
            session.phase = Phase::Choosing;
            session.note("Several test binaries were built: choose one, or stop");
            if let Err(error) = choose_target(editor, id, choices) {
                editor.set_error(error.0);
            }
        }
    }
}

/// Asks which of the test binaries built to debug.
fn choose_target(editor: &mut Editor, id: u64, choices: Vec<(String, Target)>) -> CommandResult {
    let labels: Vec<String> = choices.iter().map(|(label, _)| label.clone()).collect();
    let prompt = Prompt {
        label: "Debug tests of".into(),
        require_match: true,
        ..Prompt::default()
    };
    editor.read(
        prompt,
        move |_, input| {
            let candidates = labels.iter().map(|label| Candidate::new(label.clone()));
            filter_candidates(input, candidates.collect(), false)
        },
        move |editor, answer| {
            let (_, target) = choices
                .into_iter()
                .find(|(label, _)| *label == answer)
                .ok_or("No such test binary")?;
            if find_session(editor, id)
                .is_some_and(|session| matches!(session.phase, Phase::Choosing))
            {
                start_adapter(editor, id, target);
            }
            Ok(())
        },
    )
}

/// Gets the session's adapter, downloaded if need be, and has it launch `target`.
fn start_adapter(editor: &mut Editor, id: u64, target: Target) {
    let Some(session) = find_session(editor, id) else {
        return;
    };
    session.phase = Phase::Starting;
    let tool = session.adapter.tool();
    remacs_tools::ensure(editor, tool, move |editor, adapter| match adapter {
        Ok(adapter) => connect(editor, id, &adapter, target),
        Err(error) => {
            if let Some(session) = find_session(editor, id) {
                session.note(&format!("Could not get {tool}: {error}"));
            }
            finish(editor, id, Some(Exit::NotStarted));
        }
    });
}

/// Starts the adapter at `program` and asks it to launch `target`.
fn connect(editor: &mut Editor, id: u64, program: &Path, target: Target) {
    let notifier = editor.notifier();
    let Some(session) = find_session(editor, id) else {
        return;
    };
    if !matches!(session.phase, Phase::Starting) {
        return;
    }
    let connection = match session.adapter {
        Adapter::CodeLldb => Connection::stdio(program, &codelldb_args(), &target.cwd, notifier),
        Adapter::JsDebug => free_port().and_then(|port| {
            let server = Command::new("node")
                .arg(program)
                .arg(port.to_string())
                .arg("127.0.0.1")
                .current_dir(&target.cwd)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;
            session.port = Some(port);
            Ok(Connection::tcp(port, Some(server), notifier))
        }),
    };
    let connection = match connection {
        Ok(connection) => connection,
        Err(error) => {
            session.note(&format!(
                "Could not start {}: {error}",
                session.adapter.tool()
            ));
            finish(editor, id, Some(Exit::NotStarted));
            return;
        }
    };
    let arguments = session
        .adapter
        .launch_arguments(&session.name, &target, session.rust);
    session.phase = Phase::Debugging;
    session.shared.set_state("debugging", Tone::Success);
    add_link(editor, id, connection, ("launch".into(), arguments));
}

/// codelldb's settings: its console evaluates expressions rather than running LLDB
/// commands, as IntelliJ's does.
fn codelldb_args() -> [String; 2] {
    let settings = json!({ "consoleMode": "evaluate" });
    ["--settings".to_owned(), settings.to_string()]
}

fn add_link(editor: &mut Editor, id: u64, connection: Connection, opening: (String, Value)) {
    let Some(session) = find_session(editor, id) else {
        return;
    };
    session.links.push(Link {
        connection,
        pending: HashMap::new(),
        request: opening,
        stopped: None,
        frames: Vec::new(),
        frame: 0,
        inspected: Inspected::default(),
    });
    let address = Address {
        session: id,
        link: session.links.len() - 1,
    };
    let adapter_id = session.adapter.id();
    let initialize = json!({
        "clientID": "remacs",
        "clientName": "remacs",
        "adapterID": adapter_id,
        "locale": "en",
        "linesStartAt1": true,
        "columnsStartAt1": true,
        "pathFormat": "path",
        "supportsVariableType": true,
        "supportsRunInTerminalRequest": false,
        "supportsStartDebuggingRequest": true,
    });
    request(
        editor,
        address,
        "initialize",
        initialize,
        |editor, address, result| {
            if let Err(error) = result {
                fail(
                    editor,
                    address,
                    &format!("The debug adapter did not start: {error}"),
                );
                return;
            }
            let Some((command, arguments)) =
                find_link(editor, address).map(|link| link.request.clone())
            else {
                return;
            };
            request(
                editor,
                address,
                &command,
                arguments,
                |editor, address, result| {
                    if let Err(error) = result {
                        fail(editor, address, &error);
                    }
                },
            );
        },
    );
}

/// Sends a request; `then` gets its response.
pub fn request(
    editor: &mut Editor,
    address: Address,
    command: &str,
    arguments: Value,
    then: impl FnOnce(&mut Editor, Address, Result<Value, String>) + 'static,
) {
    let Some(link) = find_link(editor, address) else {
        return;
    };
    let seq = link.connection.request(command, arguments);
    link.pending.insert(seq, Box::new(then));
}

fn receive(editor: &mut Editor, id: u64) {
    let links = find_session(editor, id).map_or(0, |session| session.links.len());
    for index in 0..links {
        let address = Address {
            session: id,
            link: index,
        };
        loop {
            let Some(incoming) =
                find_link(editor, address).and_then(|link| link.connection.try_receive())
            else {
                break;
            };
            match incoming {
                Incoming::Message(message) => handle(editor, address, message),
                // Shown only if the adapter goes away, as the reason.
                Incoming::Log(line) => {
                    if let Some(session) = find_session(editor, id) {
                        session.last_log = Some(line);
                    }
                }
                Incoming::Closed(error) => {
                    if address.link == 0 {
                        if let Some(session) = find_session(editor, id) {
                            let unexpected = session.exit_code.is_none()
                                && !matches!(session.phase, Phase::Stopping(_));
                            if let Some(why) =
                                error.or(session.last_log.take()).filter(|_| unexpected)
                            {
                                session.note(&why);
                            }
                        }
                        finish(editor, id, None);
                    }
                    return;
                }
            }
        }
    }
}

fn handle(editor: &mut Editor, address: Address, message: Value) {
    match message["type"].as_str() {
        Some("response") => {
            let seq = message["request_seq"].as_i64().unwrap_or_default();
            let Some(handler) =
                find_link(editor, address).and_then(|link| link.pending.remove(&seq))
            else {
                return;
            };
            let result = if message["success"].as_bool().unwrap_or(false) {
                Ok(message["body"].clone())
            } else {
                Err(message["message"]
                    .as_str()
                    .or_else(|| message["body"]["error"]["format"].as_str())
                    .unwrap_or("the request failed")
                    .to_owned())
            };
            handler(editor, address, result);
        }
        Some("event") => event(editor, address, &message),
        Some("request") => reverse_request(editor, address, &message),
        _ => {}
    }
}

fn event(editor: &mut Editor, address: Address, message: &Value) {
    let body = &message["body"];
    match message["event"].as_str().unwrap_or_default() {
        "initialized" => configure(editor, address),
        "output" => output(editor, address.session, body),
        "stopped" => stopped(editor, address, body),
        "continued" => continued(editor, address),
        "exited" => {
            if let Some(session) = find_session(editor, address.session) {
                session.exit_code = body["exitCode"]
                    .as_i64()
                    .and_then(|code| code.try_into().ok());
            }
        }
        "terminated" if address.link == 0 => finish(editor, address.session, None),
        "breakpoint" => debugger(editor).breakpoints.changed(&body["breakpoint"]),
        _ => {}
    }
}

/// Sends the breakpoints, then says configuring is done.
fn configure(editor: &mut Editor, address: Address) {
    let files = debugger(editor).breakpoints.paths();
    for file in files {
        send_breakpoints(editor, address, &file);
    }
    request(
        editor,
        address,
        "setExceptionBreakpoints",
        json!({ "filters": [] }),
        |_, _, _| {},
    );
    request(
        editor,
        address,
        "configurationDone",
        json!({}),
        |_, _, _| {},
    );
}

/// Tells the adapter `file`'s breakpoints, and takes what it says of them.
pub fn send_breakpoints(editor: &mut Editor, address: Address, file: &Path) {
    let arguments = debugger(editor).breakpoints.request(file);
    let file = file.to_path_buf();
    request(
        editor,
        address,
        "setBreakpoints",
        arguments,
        move |editor, _, result| {
            if let Ok(body) = result
                && let Some(answers) = body["breakpoints"].as_array()
            {
                debugger(editor).breakpoints.verify(&file, answers);
            }
        },
    );
}

/// Sends `file`'s breakpoints to every connection debugging.
pub fn breakpoints_changed(editor: &mut Editor, file: &Path) {
    let addresses: Vec<Address> = debugger(editor)
        .sessions
        .iter()
        .filter(|session| matches!(session.phase, Phase::Debugging))
        .flat_map(|session| {
            (0..session.links.len()).map(|link| Address {
                session: session.id,
                link,
            })
        })
        .collect();
    for address in addresses {
        send_breakpoints(editor, address, file);
    }
}

fn output(editor: &mut Editor, id: u64, body: &Value) {
    let Some(session) = find_session(editor, id) else {
        return;
    };
    let text = body["output"].as_str().unwrap_or_default();
    match body["category"].as_str().unwrap_or("console") {
        "stdout" | "stderr" => session.say(Event::Output(text.replace("\r\n", "\n").into_bytes())),
        "telemetry" => {}
        _ => session.note(text),
    }
}

fn stopped(editor: &mut Editor, address: Address, body: &Value) {
    let Some(session) = find_session(editor, address.session) else {
        return;
    };
    session.shared.set_state("paused", Tone::Warning);
    let Some(link) = session.link_mut(address.link) else {
        return;
    };
    let thread = body["threadId"].as_i64().unwrap_or_default();
    link.stopped = Some(Stop {
        thread,
        reason: body["reason"].as_str().unwrap_or("pause").to_owned(),
        description: body["description"].as_str().map(str::to_owned),
    });
    link.frames.clear();
    let shown = std::mem::take(&mut link.inspected).values();
    let debugger = debugger(editor);
    if !shown.is_empty() {
        debugger.view.previous = shown;
    }
    debugger.current = Some(address);
    // A stop takes the pane slot from whatever had it, e.g. a terminal.
    editor.select_pane(crate::pane::NAME);
    let arguments = json!({ "threadId": thread, "startFrame": 0, "levels": 64 });
    request(
        editor,
        address,
        "stackTrace",
        arguments,
        |editor, address, result| {
            let Ok(body) = result else {
                return;
            };
            let Some(root) =
                find_session(editor, address.session).map(|session| session.root.clone())
            else {
                return;
            };
            let frames: Vec<Frame> = body["stackFrames"]
                .as_array()
                .map(|frames| frames.iter().map(|value| frame(value, &root)).collect())
                .unwrap_or_default();
            let Some(link) = find_link(editor, address) else {
                return;
            };
            link.frames = frames;
            // The first frame of the program's own code, past those of libraries.
            link.frame = link
                .frames
                .iter()
                .position(|frame| !frame.subtle)
                .unwrap_or(0);
            show_frame(editor, address);
            inspect::load(editor, address);
        },
    );
}

/// A frame of the stack; those outside the project at `root` are of libraries.
fn frame(value: &Value, root: &Path) -> Frame {
    let path = value["source"]["path"].as_str().map(PathBuf::from);
    let number = |key: &str| {
        value[key]
            .as_u64()
            .and_then(|number| usize::try_from(number).ok())
            .unwrap_or(1)
    };
    Frame {
        id: value["id"].as_i64().unwrap_or_default(),
        name: value["name"].as_str().unwrap_or_default().to_owned(),
        line: number("line"),
        column: number("column"),
        subtle: value["presentationHint"] == "subtle"
            || value["source"]["presentationHint"] == "deemphasize"
            || !path.as_deref().is_some_and(|path| path.starts_with(root)),
        path,
    }
}

/// Visits the selected frame's source at its line.
pub fn show_frame(editor: &mut Editor, address: Address) {
    let Some(link) = find_link(editor, address) else {
        return;
    };
    let Some(frame) = link.frames.get(link.frame).cloned() else {
        return;
    };
    let Some(path) = frame.path.filter(|path| path.is_file()) else {
        return;
    };
    if editor.visit_file(&path).is_ok() {
        editor.set_point_at(frame.line.saturating_sub(1), frame.column.saturating_sub(1));
    }
}

fn continued(editor: &mut Editor, address: Address) {
    let Some(session) = find_session(editor, address.session) else {
        return;
    };
    session.shared.set_state("debugging", Tone::Success);
    if let Some(link) = session.link_mut(address.link) {
        link.stopped = None;
        link.frames.clear();
        let shown = std::mem::take(&mut link.inspected).values();
        if !shown.is_empty() {
            debugger(editor).view.previous = shown;
        }
    }
}

/// Steps or goes on from where the connection stopped.
pub fn resume(editor: &mut Editor, address: Address, command: &str) -> Result<(), &'static str> {
    let thread = find_link(editor, address)
        .and_then(|link| link.stopped.as_ref())
        .map(|stop| stop.thread)
        .ok_or("The program is not paused")?;
    continued(editor, address);
    request(
        editor,
        address,
        command,
        json!({ "threadId": thread }),
        |editor, address, result| {
            if let Err(error) = result {
                fail(editor, address, &error);
            }
        },
    );
    Ok(())
}

/// Pauses every thread.
pub fn pause(editor: &mut Editor, address: Address) {
    request(
        editor,
        address,
        "threads",
        json!({}),
        |editor, address, result| {
            let thread = result
                .ok()
                .and_then(|body| body["threads"][0]["id"].as_i64())
                .unwrap_or(1);
            request(
                editor,
                address,
                "pause",
                json!({ "threadId": thread }),
                |_, _, _| {},
            );
        },
    );
}

/// js-debug asks for a session of each process it debugs, on a connection of its own.
fn reverse_request(editor: &mut Editor, address: Address, message: &Value) {
    let notifier = editor.notifier();
    let Some(session) = find_session(editor, address.session) else {
        return;
    };
    let Some(parent) = session.link(address.link) else {
        return;
    };
    if message["command"] != "startDebugging" {
        parent.connection.respond(
            message,
            Err(format!("{} is not supported", message["command"])),
        );
        return;
    }
    let Some(port) = session.port else {
        parent
            .connection
            .respond(message, Err("no port to connect to".into()));
        return;
    };
    parent.connection.respond(message, Ok(json!({})));
    let arguments = &message["arguments"];
    let command = arguments["request"].as_str().unwrap_or("launch").to_owned();
    let connection = Connection::tcp(port, None, notifier);
    add_link(
        editor,
        address.session,
        connection,
        (command, arguments["configuration"].clone()),
    );
}

fn fail(editor: &mut Editor, address: Address, error: &str) {
    if let Some(session) = find_session(editor, address.session) {
        session.note(error);
    }
    if address.link == 0 {
        finish(editor, address.session, Some(Exit::NotStarted));
    }
}

/// Ends the session: the console is told how the program ended, and the adapter goes.
fn finish(editor: &mut Editor, id: u64, exit: Option<Exit>) {
    let debugger = debugger(editor);
    let Some(index) = debugger
        .sessions
        .iter()
        .position(|session| session.id == id)
    else {
        return;
    };
    let session = debugger.sessions.remove(index);
    if debugger
        .current
        .is_some_and(|current| current.session == id)
    {
        debugger.current = None;
    }
    let exit = exit.unwrap_or(Exit::Code(session.exit_code.unwrap_or(0)));
    session.say(Event::Exited(exit));
}

/// The connection stepping acts on: the one that stopped last, else the only one there is.
/// The frame of the connection that stopped last, as selected.
pub fn current_frame(editor: &Editor) -> Option<&Frame> {
    let debugger = editor.find_plugin_state::<Debugger>()?;
    let address = debugger.current?;
    let session = debugger
        .sessions
        .iter()
        .find(|session| session.id == address.session)?;
    let link = session.link(address.link)?;
    link.stopped.as_ref()?;
    link.frames.get(link.frame)
}

pub fn current(editor: &mut Editor) -> Option<Address> {
    let debugger = debugger(editor);
    debugger.current.or_else(|| {
        let session = debugger.sessions.last()?;
        Some(Address {
            session: session.id,
            link: session.links.len().checked_sub(1)?,
        })
    })
}
