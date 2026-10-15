//! The run of the window buffer's project in the status pill, and a console's Stop and
//! Rerun buttons.

use std::rc::Rc;
use std::time::Duration;

use remacs_core::{Editor, Icon, StatusItem, StatusSegment, TextControls, Tone, ToolbarButton};

use crate::CHOOSE;
use crate::commands::{run, stop_console};
use crate::configuration::Store;
use crate::process::Exit;
use crate::sessions::{self, Session, State, get, of_window};
use crate::window::window_root;

const CONSOLE_HINTS: &[(&str, &str)] = &[
    ("↵", "Open link / input"),
    ("TAB", "Next link"),
    ("C-c C-c", "Stop"),
    ("g", "Rerun"),
    ("M-g M-n", "Next error"),
    ("M-1…9", "Run"),
];

/// The console's configuration, else the project's chosen one: its name and how its
/// last run went. A click chooses another.
pub fn status(editor: &Editor) -> Option<StatusSegment> {
    let (configuration, session) = match of_window(editor).and_then(|id| get(editor, id)) {
        Some(session) => (session.configuration.clone(), Some(session)),
        None => {
            let root = window_root(editor)?;
            let store = editor.find_plugin_state::<Store>()?;
            let name = store.selected(&root)?;
            let configuration = store.read(&root)?.find(name)?.clone();
            let session = sessions::find(editor, &root, name).and_then(|id| get(editor, id));
            (configuration, session)
        }
    };
    let mut items = vec![StatusItem::new(
        Some(configuration.kind.icon()),
        configuration.name,
    )];
    items.push(match session {
        None => StatusItem::toned("idle", Tone::Faint),
        Some(session) => state_item(session),
    });
    Some(StatusSegment {
        items,
        command: Some(CHOOSE),
        active: false,
        code: true,
    })
}

fn state_item(session: &Session) -> StatusItem {
    if session.state == State::Running
        && let Some((text, tone)) = session.program_state()
    {
        return StatusItem::toned(text, tone);
    }
    let (text, tone) = match session.state {
        State::Running => (
            format!("running {}", clock(session.elapsed())),
            Tone::Success,
        ),
        State::Stopping => ("stopping".to_owned(), Tone::Warning),
        State::Stopped => ("stopped".to_owned(), Tone::Faint),
        State::Exited(Exit::Code(0)) => ("exited 0".to_owned(), Tone::Faint),
        State::Exited(Exit::Code(code)) => (format!("exited {code}"), Tone::Danger),
        State::Exited(Exit::Signal(signal)) => (format!("signal {signal}"), Tone::Danger),
        State::Exited(Exit::NotStarted) => ("failed to start".to_owned(), Tone::Danger),
    };
    StatusItem::toned(text, tone)
}

/// `3:07`, or `1:02:03` past an hour.
fn clock(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Stop and Rerun over a console.
pub fn controls(editor: &Editor) -> Option<TextControls> {
    let session = get(editor, of_window(editor)?)?;
    Some(TextControls {
        toolbar: vec![
            ToolbarButton {
                icon: Icon::Stop,
                label: "Stop".into(),
                count: None,
                enabled: session.is_running(),
                action: Rc::new(stop_console),
            },
            ToolbarButton {
                icon: Icon::Rerun,
                label: "Rerun".into(),
                count: None,
                enabled: true,
                action: Rc::new(run),
            },
        ],
        hints: CONSOLE_HINTS,
        ..TextControls::default()
    })
}
