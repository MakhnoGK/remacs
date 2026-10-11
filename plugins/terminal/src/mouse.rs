//! The mouse over a terminal: reported to programs that ask for it, in the encoding they
//! ask for; else it selects, the wheel scrolls, and ⌘ follows links. Shift keeps the
//! mouse for remacs whatever the program asked.

use alacritty_terminal::Term;
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;
use remacs_core::{ScreenButton, ScreenMouse, ScreenPointer};
use remacs_files::links::Target;

use crate::links::link_at;
use crate::session::Listener;
use crate::terminal::Terminal;

/// Wheel turned up, as xterm reports it; down is the next.
const WHEEL_UP: u8 = 64;
/// Added to a button for a move with it held.
const MOTION: u8 = 32;
/// A move with no button held, and a release where the encoding cannot tell which.
const NO_BUTTON: u8 = 3;
/// Cells past which the oldest encoding cannot report the mouse.
const NORMAL_LIMIT: usize = 223;

#[derive(Default)]
pub struct MouseState {
    gesture: Gesture,
    /// The cell last reported, so moves within it are not.
    reported: Option<(usize, usize)>,
}

/// What the button held does.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Gesture {
    #[default]
    None,
    Select,
    Report(ScreenButton),
}

pub enum Outcome {
    /// Whether the screen changed.
    Changed(bool),
    Follow(Target),
}

pub fn take(terminal: &mut Terminal, pointer: ScreenPointer) -> Outcome {
    let Some(session) = &terminal.session else {
        return Outcome::Changed(false);
    };
    let mut term = session.screen.lock();
    let offset = term.grid().display_offset() as i32;
    let point = Point::new(Line(pointer.row as i32 - offset), Column(pointer.column));
    let following = pointer.modifiers.super_key && pointer.mouse != ScreenMouse::Leave;
    let link = following
        .then(|| link_at(&term, point, &terminal.directory))
        .flatten();
    let hover_changed = terminal.link != link;
    terminal.link = link;
    let mode = *term.mode();
    let reporting = mode.intersects(TermMode::MOUSE_MODE) && !pointer.modifiers.shift;
    let cell = Some((pointer.row, pointer.column));
    let state = &mut terminal.mouse;
    let mut bytes = Vec::new();
    match pointer.mouse {
        ScreenMouse::Press { button, clicks } => {
            if let Some(link) = terminal
                .link
                .as_ref()
                .filter(|_| button == ScreenButton::Left)
            {
                return Outcome::Follow(link.target.clone());
            }
            state.gesture = if reporting {
                state.reported = cell;
                bytes = report(mode, code(button), &pointer, false);
                Gesture::Report(button)
            } else if button == ScreenButton::Left {
                let extend = pointer.modifiers.shift && !mode.intersects(TermMode::MOUSE_MODE);
                select(&mut term, point, clicks, extend);
                Gesture::Select
            } else {
                Gesture::None
            };
        }
        ScreenMouse::Drag => match state.gesture {
            Gesture::Report(button)
                if mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION)
                    && state.reported != cell =>
            {
                state.reported = cell;
                bytes = report(mode, code(button) + MOTION, &pointer, false);
            }
            Gesture::Select => {
                if let Some(selection) = &mut term.selection {
                    selection.update(point, Side::Right);
                }
            }
            _ => {}
        },
        ScreenMouse::Release { button } => match std::mem::take(&mut state.gesture) {
            Gesture::Report(pressed) if pressed == button => {
                bytes = report(mode, code(button), &pointer, true);
            }
            Gesture::Select if term.selection.as_ref().is_some_and(Selection::is_empty) => {
                term.selection = None;
            }
            _ => {}
        },
        ScreenMouse::Move => {
            if reporting && mode.contains(TermMode::MOUSE_MOTION) && state.reported != cell {
                state.reported = cell;
                bytes = report(mode, NO_BUTTON + MOTION, &pointer, false);
            }
        }
        ScreenMouse::Leave => {}
        ScreenMouse::Wheel { rows } => {
            let up = rows < 0;
            if reporting {
                let code = WHEEL_UP + u8::from(!up);
                for _ in 0..rows.unsigned_abs() {
                    bytes.extend(report(mode, code, &pointer, false));
                }
            } else if !pointer.modifiers.shift
                && mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL)
            {
                let arrow = match (up, mode.contains(TermMode::APP_CURSOR)) {
                    (true, true) => "\x1bOA",
                    (true, false) => "\x1b[A",
                    (false, true) => "\x1bOB",
                    (false, false) => "\x1b[B",
                };
                bytes = arrow.repeat(rows.unsigned_abs()).into_bytes();
            } else {
                term.scroll_display(Scroll::Delta(-rows as i32));
            }
        }
    }
    drop(term);
    if !bytes.is_empty() {
        session.write(bytes);
    }
    let moved = matches!(pointer.mouse, ScreenMouse::Move | ScreenMouse::Leave);
    Outcome::Changed(hover_changed || !moved)
}

/// A click selects from the cell, a double click the word, a triple one the line;
/// `extend` stretches the selection there is.
fn select(term: &mut Term<Listener>, point: Point, clicks: usize, extend: bool) {
    if extend && let Some(selection) = &mut term.selection {
        selection.update(point, Side::Right);
        return;
    }
    let kind = match clicks {
        1 => SelectionType::Simple,
        2 => SelectionType::Semantic,
        _ => SelectionType::Lines,
    };
    term.selection = Some(Selection::new(kind, point, Side::Left));
}

fn code(button: ScreenButton) -> u8 {
    match button {
        ScreenButton::Left => 0,
        ScreenButton::Middle => 1,
        ScreenButton::Right => 2,
    }
}

/// The mouse as xterm reports it: SGR (1006), UTF-8 (1005) or the oldest encoding,
/// which cannot reach past column or row 223, where nothing is reported.
fn report(mode: TermMode, code: u8, pointer: &ScreenPointer, release: bool) -> Vec<u8> {
    let held = pointer.modifiers;
    let code =
        code + 4 * u8::from(held.shift) + 8 * u8::from(held.meta) + 16 * u8::from(held.control);
    let (column, row) = (pointer.column + 1, pointer.row + 1);
    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if release { 'm' } else { 'M' };
        return format!("\x1b[<{code};{column};{row}{end}").into_bytes();
    }
    let code = 32 + if release { code & !3 | NO_BUTTON } else { code };
    if mode.contains(TermMode::UTF8_MOUSE) {
        let mut report = String::from("\x1b[M");
        for value in [usize::from(code), 32 + column, 32 + row] {
            match u32::try_from(value).ok().and_then(char::from_u32) {
                Some(character) => report.push(character),
                None => return Vec::new(),
            }
        }
        return report.into_bytes();
    }
    if column > NORMAL_LIMIT || row > NORMAL_LIMIT {
        return Vec::new();
    }
    vec![0x1b, b'[', b'M', code, 32 + column as u8, 32 + row as u8]
}

#[cfg(test)]
mod tests {
    use remacs_core::Modifiers;

    use super::*;

    fn pointer(row: usize, column: usize, control: bool) -> ScreenPointer {
        ScreenPointer {
            row,
            column,
            modifiers: Modifiers {
                control,
                ..Modifiers::default()
            },
            mouse: ScreenMouse::Move,
        }
    }

    #[test]
    fn reports_the_mouse_as_asked() {
        let sgr = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(
            report(sgr, 0, &pointer(4, 9, false), false),
            b"\x1b[<0;10;5M"
        );
        assert_eq!(
            report(sgr, 2, &pointer(4, 9, false), true),
            b"\x1b[<2;10;5m"
        );
        assert_eq!(
            report(sgr, 65, &pointer(0, 0, true), false),
            b"\x1b[<81;1;1M"
        );
        let normal = TermMode::MOUSE_REPORT_CLICK;
        assert_eq!(
            report(normal, 0, &pointer(0, 0, false), false),
            b"\x1b[M !!"
        );
        assert_eq!(report(normal, 2, &pointer(0, 0, false), true), b"\x1b[M#!!");
        assert_eq!(report(normal, 0, &pointer(0, 300, false), false), b"");
        let utf8 = TermMode::MOUSE_REPORT_CLICK | TermMode::UTF8_MOUSE;
        assert_eq!(
            report(utf8, 0, &pointer(0, 300, false), false),
            "\x1b[M \u{14d}!".as_bytes()
        );
    }
}
