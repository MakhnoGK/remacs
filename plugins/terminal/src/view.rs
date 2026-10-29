//! How terminals are drawn and take input: as screens, in the panel with chips, in the
//! window with chips in the title bar, in the status pill, and keys and mouse.

use std::rc::Rc;
use std::time::Duration;

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::term::TermMode;
use remacs_core::{
    Buffer, CommandResult, Editor, Key, KeyCode, Pane, PaneBody, PaneButton, Screen, ScreenBanner,
    ScreenMouse, ScreenNotice, ScreenPadding, ScreenPointer, ScreenSize, ScreenSource, StatusItem,
    StatusSegment, Tab, Tabs, TextControls, Tone,
};
use remacs_files::abbreviate_home;
use remacs_files::links::follow;

use crate::copy::{self, Outcome};
use crate::grid::{Overlay, rows};
use crate::history::Commands;
use crate::keys::encode;
use crate::links::link_at;
use crate::mouse;
use crate::session::Phase;
use crate::terminal::Terminal;
use crate::terminals::{
    PANE, all, find, find_mut, in_panel, in_window, is_ended, keyed, settings, show_here, terminals,
};

pub const LIST: &str = "terminal-list";
pub const NEW: &str = "terminal-new";
pub const MAXIMIZE: &str = "terminal-maximize";
pub const CLOSE: &str = "terminal-close";

// Short enough to stay clear of the status pill beside them.
const PANEL_HINTS: &[(&str, &str)] = &[
    ("C-`", "Hide"),
    ("⌘↩", "Maximize"),
    ("C-x o", "Code"),
    ("C-x [", "Copy mode"),
];
const PANEL_UNFOCUSED_HINTS: &[(&str, &str)] =
    &[("C-`", "Focus terminal"), ("C-x o", "Next window")];
const WINDOW_HINTS: &[(&str, &str)] = &[
    ("⌘↩", "To panel"),
    ("⌘T", "New"),
    ("⌘{ ⌘}", "Switch"),
    ("C-x [", "Copy mode"),
];
const EXITED_HINTS: &[(&str, &str)] = &[("↵", "Restart"), ("q", "Close"), ("C-x [", "Copy mode")];
const FAILED_HINTS: &[(&str, &str)] = &[
    ("↵", "Open config"),
    (
        "z",
        if cfg!(target_os = "macos") {
            "Start zsh once"
        } else {
            "Start sh once"
        },
    ),
    ("r", "Retry"),
];
const COPY_HINTS: &[(&str, &str)] = &[
    ("C-s", "Search"),
    ("C-SPC", "Mark"),
    ("M-w", "Copy"),
    ("q", "Leave"),
];
const EXITED_KEYS: &[(&str, &str)] = &[("↵", "restart"), ("q", "close")];
/// How long the screen flashes when the bell rings.
pub const FLASH: Duration = Duration::from_millis(150);

pub struct TerminalScreens;

impl ScreenSource for TerminalScreens {
    fn draws(&self, buffer: &Buffer) -> bool {
        buffer.local::<Terminal>().is_some()
    }

    fn screen(&self, editor: &Editor, buffer: &Buffer, size: ScreenSize) -> Option<Screen> {
        let terminal = buffer.local::<Terminal>()?;
        if let Some(session) = &terminal.session {
            session.resize(size);
        }
        let notice = notice(terminal);
        let Some(session) = terminal.session.as_ref().filter(|_| notice.is_none()) else {
            return Some(Screen {
                notice,
                ..Screen::default()
            });
        };
        let term = session.screen.lock();
        let (matches, current_match) =
            match terminal.copy.as_ref().and_then(|copy| copy.search.as_ref()) {
                Some(search) => (
                    search.matches.as_slice(),
                    search
                        .current
                        .and_then(|current| search.matches.get(current)),
                ),
                None => (&[][..], None),
            };
        let overlay = Overlay {
            focused: keyed(editor) == Some(terminal.id),
            matches,
            current_match,
            commands: &terminal.commands,
            link: terminal.link.as_ref().map(|link| &link.cells),
        };
        let (mut rows, cursor) = rows(&term, &overlay);
        if program_has_screen(*term.mode(), &terminal.commands) {
            rows.iter_mut().for_each(|row| row.mark = None);
        }
        let banner = match terminal.phase {
            Phase::Exited(code) => Some(ScreenBanner {
                text: format!("Process exited with code {code}"),
                tone: if code == 0 {
                    Tone::Success
                } else {
                    Tone::Danger
                },
                keys: EXITED_KEYS,
            }),
            _ => None,
        };
        let copying = terminal.copy.as_ref();
        Some(Screen {
            rows,
            cursor,
            banner,
            notice: None,
            mode: copying.map(|_| copy::mode(&term)),
            search: copying
                .and_then(|copy| copy.search.as_ref())
                .map(copy::search_bar),
            above: term.grid().history_size() > term.grid().display_offset(),
            flash: terminal.rang.is_some_and(|rang| rang.elapsed() < FLASH),
            pointing: terminal.link.is_some(),
        })
    }

    fn padding(&self, editor: &Editor) -> ScreenPadding {
        let settings = settings(editor);
        ScreenPadding {
            horizontal: settings.padding_x,
            vertical: settings.padding_y,
        }
    }

    fn mouse(&self, editor: &mut Editor, buffer: &str, pointer: ScreenPointer) -> bool {
        let Some(id) = terminal_named(editor, buffer).map(|terminal| terminal.id) else {
            return false;
        };
        if let ScreenMouse::Press { .. } = pointer.mouse {
            focus(editor, id);
        }
        let Some(terminal) = find_mut(editor, id) else {
            return false;
        };
        let directory = terminal.directory.clone();
        match mouse::take(terminal, pointer) {
            mouse::Outcome::Changed(changed) => changed,
            mouse::Outcome::Follow(target) => {
                let from_window = in_window(editor) == Some(id);
                if let Err(error) = follow(editor, &target, &[directory], from_window) {
                    editor.set_error(error.0);
                }
                true
            }
        }
    }
}

fn terminal_named<'a>(editor: &'a Editor, buffer: &str) -> Option<&'a Terminal> {
    editor
        .buffers()
        .iter()
        .find(|candidate| candidate.name() == buffer)?
        .local::<Terminal>()
}

/// Gives the keys to terminal `id`, where it is shown.
fn focus(editor: &mut Editor, id: u64) {
    if in_panel(editor) == Some(id) {
        editor.select_pane(PANE);
        editor.focus_pane(true);
    } else if in_window(editor) == Some(id) {
        editor.focus_pane(false);
    }
}

/// Whether a program draws its own interface: one on the alternate screen, like vim, or a
/// command that took over keys or the mouse to draw in place, like Claude Code. Shells
/// turn these modes off before running a command, so a running one turned them on.
fn program_has_screen(mode: TermMode, commands: &Commands) -> bool {
    let interactive = TermMode::BRACKETED_PASTE
        | TermMode::FOCUS_IN_OUT
        | TermMode::MOUSE_MODE
        | TermMode::KITTY_KEYBOARD_PROTOCOL;
    mode.contains(TermMode::ALT_SCREEN) || (commands.running() && mode.intersects(interactive))
}

/// In place of the rows: the shell starting, or why it could not.
fn notice(terminal: &Terminal) -> Option<ScreenNotice> {
    let shell = terminal.shell();
    match &terminal.phase {
        Phase::Starting => Some(ScreenNotice {
            title: format!("Starting {shell}…"),
            detail: format!(
                "{} {} · in {}",
                terminal.program,
                terminal.args.join(" "),
                abbreviate_home(&terminal.directory)
            ),
            tone: Tone::Faint,
        }),
        Phase::Failed(error) => Some(ScreenNotice {
            title: if error.starts_with("no such file") {
                "Shell not found".to_owned()
            } else {
                format!("Could not start {shell}")
            },
            detail: format!(
                "remacs could not start {shell}: {error}. The shell is set in terminal.toml."
            ),
            tone: Tone::Danger,
        }),
        _ => None,
    }
}

pub fn pane_shown(editor: &Editor) -> bool {
    in_panel(editor).is_some()
}

pub fn pane(editor: &Editor) -> Option<Pane> {
    let id = in_panel(editor)?;
    let terminal = find(editor, id)?;
    let name = crate::terminals::buffer_name(editor, id)?;
    let focused = editor.pane_has_keys(PANE);
    let hints = match (&terminal.phase, &terminal.copy, focused) {
        (_, _, false) => PANEL_UNFOCUSED_HINTS,
        (_, Some(_), true) => COPY_HINTS,
        (Phase::Exited(_), _, true) => EXITED_HINTS,
        (Phase::Failed(_), _, true) => FAILED_HINTS,
        _ => PANEL_HINTS,
    };
    Some(Pane {
        count: Some(all(editor).len()),
        height: settings(editor).panel_height,
        title: String::new(),
        subtitle: format!(
            "{} · {}",
            terminal.shell(),
            abbreviate_home(&terminal.directory)
        ),
        state: state(terminal),
        tabs: Some(tabs(editor, id)),
        buttons: vec![
            button("New", "⌘T", NEW),
            button("Maximize", "⌘↩", MAXIMIZE),
            button("Close", "C-x k", CLOSE),
        ],
        body: PaneBody::Screen(name),
        hints,
    })
}

/// Chips and buttons over a terminal shown in the window.
pub fn controls(editor: &Editor) -> Option<TextControls> {
    let id = in_window(editor)?;
    let terminal = find(editor, id)?;
    let hints = match (&terminal.phase, &terminal.copy) {
        (_, Some(_)) => COPY_HINTS,
        (Phase::Exited(_), _) => EXITED_HINTS,
        (Phase::Failed(_), _) => FAILED_HINTS,
        _ => WINDOW_HINTS,
    };
    Some(TextControls {
        buttons: vec![button("Restore to panel", "⌘↩", MAXIMIZE)],
        tabs: Some(tabs(editor, id)),
        hints,
        ..TextControls::default()
    })
}

fn button(label: &'static str, keys: &'static str, command: &'static str) -> PaneButton {
    PaneButton {
        label,
        keys,
        enabled: true,
        action: Rc::new(move |editor: &mut Editor| run(editor, command)),
    }
}

fn run(editor: &mut Editor, command: &str) -> CommandResult {
    let run = editor.commands().get(command).ok_or("No such command")?.run;
    run(editor)
}

fn tabs(editor: &Editor, shown: u64) -> Tabs {
    let tabs = all(editor)
        .into_iter()
        .map(|terminal| {
            let id = terminal.id;
            Tab {
                label: terminal.label(),
                selected: id == shown,
                state: terminal.tab_state(),
                activity: terminal.activity,
                bell: terminal.bell,
                editing: None,
                action: Rc::new(move |editor: &mut Editor| {
                    show_here(editor, id);
                    Ok(())
                }),
            }
        })
        .collect();
    Tabs {
        tabs,
        list: LIST,
        add: NEW,
    }
}

fn state(terminal: &Terminal) -> (String, Tone) {
    match &terminal.phase {
        Phase::Starting => ("starting".to_owned(), Tone::Faint),
        Phase::Running => match terminal.tab_state() {
            remacs_core::TabState::Running => ("running".to_owned(), Tone::Success),
            _ => ("idle".to_owned(), Tone::Faint),
        },
        Phase::Exited(0) => ("exited 0".to_owned(), Tone::Faint),
        Phase::Exited(code) => (format!("exited {code}"), Tone::Danger),
        Phase::Failed(_) => ("failed".to_owned(), Tone::Danger),
    }
}

/// The shell, where it is and the screen's size, for the terminal in the window. The
/// panel's header says as much, and the pill stays the text's whether or not the panel has
/// the keys, so that it does not change as they move.
pub fn status(editor: &Editor) -> Option<StatusSegment> {
    let id = in_window(editor)?;
    let terminal = find(editor, id)?;
    let mut items = vec![StatusItem::new(None, terminal.shell())];
    items.push(StatusItem::new(None, abbreviate_home(&terminal.directory)));
    let last = match (&terminal.phase, &terminal.session) {
        (Phase::Failed(_), _) => StatusItem::toned("not found", Tone::Danger),
        (Phase::Exited(code), _) => StatusItem::toned(format!("exited {code}"), Tone::Faint),
        (Phase::Starting, _) => StatusItem::toned("starting", Tone::Faint),
        (_, Some(session)) => {
            let size = session.size();
            StatusItem::new(None, format!("{}×{}", size.columns, size.rows))
        }
        (_, None) => return None,
    };
    items.push(last);
    Some(StatusSegment {
        items,
        command: None,
        active: false,
        code: true,
    })
}

/// Passes a key to the program of the terminal with the keys, unless remacs keeps it.
pub fn take_key(editor: &mut Editor, key: Key) -> bool {
    let Some(id) = keyed(editor) else {
        return false;
    };
    let kept = key.modifiers.super_key || terminals(editor).kept_keys.contains(&key);
    let Some(terminal) = find_mut(editor, id) else {
        return false;
    };
    if terminal.copy.is_some() {
        return copy_key(editor, id, key, kept);
    }
    if kept || is_ended(terminal) {
        return false;
    }
    let Some(session) = &terminal.session else {
        return false;
    };
    let mut term = session.screen.lock();
    let Some(bytes) = encode(key, *term.mode()) else {
        return false;
    };
    term.scroll_display(Scroll::Bottom);
    term.selection = None;
    drop(term);
    session.write(bytes);
    true
}

fn copy_key(editor: &mut Editor, id: u64, key: Key, kept: bool) -> bool {
    let Some(terminal) = find_mut(editor, id) else {
        return false;
    };
    let (Some(copy), Some(session)) = (terminal.copy.as_mut(), &terminal.session) else {
        return false;
    };
    let mut term = session.screen.lock();
    let directory = terminal.directory.clone();
    let link = (copy.search.is_none() && key.code == KeyCode::Return)
        .then(|| link_at(&term, term.vi_mode_cursor.point, &directory))
        .flatten();
    let outcome = match link {
        Some(link) => Outcome::Follow(link.target),
        None => copy::take_key(copy, &mut term, key),
    };
    let leave = matches!(
        outcome,
        Outcome::Copy(_) | Outcome::Leave | Outcome::Follow(_)
    );
    if leave {
        copy::leave(&mut term);
    }
    drop(term);
    if leave {
        terminal.copy = None;
    }
    match outcome {
        Outcome::Copy(text) => {
            editor.kill_ring.push(text);
            editor.set_done("Copied");
            true
        }
        Outcome::Follow(target) => {
            let from_window = in_window(editor) == Some(id);
            if let Err(error) = follow(editor, &target, &[directory], from_window) {
                editor.set_error(error.0);
            }
            true
        }
        Outcome::Handled | Outcome::Leave => true,
        // Typed characters do nothing here; remacs's own keys still work.
        Outcome::Ignored => !kept,
    }
}

/// While a terminal has the keys, characters typed never reach the text.
pub fn has_keys(editor: &Editor) -> bool {
    keyed(editor).is_some()
}

/// The terminal with the keys has ended.
pub fn ended_has_keys(editor: &Editor) -> bool {
    keyed(editor)
        .and_then(|id| find(editor, id))
        .is_some_and(|terminal| {
            matches!(terminal.phase, Phase::Exited(_)) && terminal.copy.is_none()
        })
}

pub fn failed_has_keys(editor: &Editor) -> bool {
    keyed(editor)
        .and_then(|id| find(editor, id))
        .is_some_and(|terminal| matches!(terminal.phase, Phase::Failed(_)))
}
