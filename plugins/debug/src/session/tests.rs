use std::sync::Arc;
use std::thread;

use super::*;

/// `run.toml`'s default `output-backlog`, and its least.
const BACKLOGS: [usize; 2] = [16, 1];
const ROUNDS: usize = 20;
const PAUSE: Duration = Duration::from_millis(5);

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

fn printed(events: &mpsc::Receiver<Event>) -> (usize, String) {
    let queued: Vec<Event> = events.try_iter().collect();
    let text = queued
        .iter()
        .filter_map(|event| match event {
            Event::Output(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
            _ => None,
        })
        .collect();
    (queued.len(), text)
}

#[test]
fn connections_take_turns_and_together_queue_no_more_than_the_console_takes() {
    for backlog in BACKLOGS {
        let links = vec![flooding("a", backlog), flooding("b", backlog)];
        let (mut editor, events) = debugging(links, backlog);
        let mut all = String::new();
        for _ in 0..ROUNDS {
            thread::sleep(PAUSE);
            update(&mut editor);
            // All of it, where the console takes a backlog a refresh.
            let (queued, text) = printed(&events);
            assert!(queued <= backlog, "{queued} queued, backlog {backlog}");
            all += &text;
        }
        assert!(
            all.contains("a\n") && all.contains("b\n"),
            "backlog {backlog}: {all}"
        );
    }
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
    thread::sleep(PAUSE * 10);
    update(&mut editor);
    let (_, text) = printed(&events);
    assert!(text.contains("c\n"), "{text}");
}
