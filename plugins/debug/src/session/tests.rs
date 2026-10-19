use std::sync::Arc;
use std::thread;

use super::*;
use crate::prepare::Plan;

/// `run.toml`'s default `output-backlog`, and its least.
const BACKLOGS: [usize; 2] = [16, 1];
const ROUNDS: usize = 20;
const PAUSE: Duration = Duration::from_millis(5);
/// Long enough for the shells started to have printed, on a machine busy with other tests.
const SETTLE: Duration = Duration::from_millis(500);
/// How long the shells have to print at all, which they take far less than.
const DEADLINE: Duration = Duration::from_secs(5);

/// An adapter run by `script`.
fn link(script: String, backlog: usize) -> Link {
    let connection = Connection::stdio(
        Path::new("/bin/sh"),
        &["-c".into(), script],
        &std::env::temp_dir(),
        backlog,
        Arc::new(|| {}),
    )
    .expect("starting the shell");
    Link {
        connection,
        pending: HashMap::new(),
        request: (String::new(), Value::Null),
        stopped: None,
        frames: Vec::new(),
        frame: 0,
        inspected: Inspected::default(),
    }
}

/// An adapter telling of a program that prints `text` a line after another.
fn flooding(text: &str, backlog: usize) -> Link {
    let body = format!(
        r#"{{"type":"event","event":"output","body":{{"category":"stdout","output":"{text}\n"}}}}"#
    );
    let script = format!(
        "yes \"$(printf 'Content-Length: {}\\r\\n\\r\\n%s' '{body}')\"",
        body.len()
    );
    link(script, backlog)
}

fn debugging(links: Vec<Link>, backlog: usize) -> (Editor, mpsc::Receiver<Event>) {
    let mut editor = Editor::new(&[]);
    let (output, events) = mpsc::channel();
    debugger(&mut editor).sessions.push(Session {
        id: 1,
        name: "flood".into(),
        root: std::env::temp_dir(),
        rust: false,
        adapter: Adapter::JsDebug,
        output,
        backlog,
        shared: Rc::default(),
        phase: Phase::Debugging,
        links,
        turn: 0,
        port: None,
        exit_code: None,
        last_log: None,
    });
    (editor, events)
}

/// The output among `events`, as text.
fn text(events: &[Event]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Output(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
            _ => None,
        })
        .collect()
}

/// How many events wait for the console, and their output.
fn printed(events: &mpsc::Receiver<Event>) -> (usize, String) {
    let queued: Vec<Event> = events.try_iter().collect();
    (queued.len(), text(&queued))
}

#[test]
fn connections_take_turns_and_together_queue_no_more_than_the_console_takes() {
    for backlog in BACKLOGS {
        let links = vec![flooding("a", backlog), flooding("b", backlog)];
        let (mut editor, events) = debugging(links, backlog);
        let deadline = Instant::now() + DEADLINE;
        let mut all = String::new();
        for round in 0.. {
            if round >= ROUNDS && all.contains("a\n") && all.contains("b\n") {
                break;
            }
            assert!(Instant::now() < deadline, "backlog {backlog}: {all}");
            thread::sleep(PAUSE);
            update(&mut editor);
            // All of it, where the console takes a backlog a refresh.
            let (queued, text) = printed(&events);
            assert!(queued <= backlog, "{queued} queued, backlog {backlog}");
            all += &text;
        }
    }
}

#[test]
fn a_finished_build_sends_all_its_output_before_it_ends() {
    let plan = Plan {
        before: vec!["echo one; echo two; exit 3".into()],
        cargo_build: None,
        target: Target {
            program: "true".into(),
            args: Vec::new(),
            cwd: std::env::temp_dir(),
            env: Vec::new(),
        },
    };
    let preparation = Preparation::start(plan, BACKLOGS[0], Arc::new(|| {}));
    // So that the first refresh finds it done, more of its output waiting than it takes.
    thread::sleep(SETTLE);
    let (mut editor, events) = debugging(Vec::new(), BACKLOGS[1]);
    debugger(&mut editor).sessions[0].phase = Phase::Preparing(preparation);
    let deadline = Instant::now() + DEADLINE;
    let mut queued = Vec::new();
    while !matches!(queued.last(), Some(Event::Exited(_))) {
        assert!(Instant::now() < deadline, "the build did not end");
        update(&mut editor);
        queued.extend(events.try_iter());
        thread::sleep(PAUSE);
    }
    assert_eq!(text(&queued), "one\ntwo\n");
    assert!(matches!(queued.last(), Some(Event::Exited(Exit::Code(3)))));
}

#[test]
fn a_connection_closing_holds_up_none_after_it() {
    let backlog = BACKLOGS[0];
    let links = vec![
        link("sleep 5".into(), backlog),
        link("exit 0".into(), backlog),
        flooding("c", backlog),
    ];
    let (mut editor, events) = debugging(links, backlog);
    // No waiting until each is ready: reading one to see would take what is tested.
    thread::sleep(SETTLE);
    update(&mut editor);
    let (_, text) = printed(&events);
    assert!(text.contains("c\n"), "{text}");
}
