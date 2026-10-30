//! Copy mode: the scrollback read like a buffer with Emacs keys, a cursor of its own, a
//! region and an incremental search, as `M-w` copies from it.

use std::ops::RangeInclusive;

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Direction, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::search::{RegexIter, RegexSearch};
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vi_mode::ViMotion;
use remacs_core::{Key, KeyCode, ScreenMode, ScreenSearch};
use remacs_files::links::Target;

use crate::session::Listener;

pub const SEARCH_HINTS: &[(&str, &str)] = &[("C-s", "next"), ("C-r", "previous"), ("RET", "stop")];

#[derive(Default)]
pub struct CopyMode {
    pub search: Option<Search>,
}

pub struct Search {
    pub input: String,
    forward: bool,
    /// Every match in the scrollback and on the screen, top to bottom.
    pub matches: Vec<RangeInclusive<Point>>,
    pub current: Option<usize>,
    /// Where the cursor was when the search started, for `C-g`.
    origin: Point,
}

/// What a key did in copy mode.
pub enum Outcome {
    Handled,
    /// Copy the region and leave.
    Copy(String),
    Leave,
    /// Leave and follow the link under the cursor.
    Follow(Target),
    /// Not a copy mode key: it does nothing.
    Ignored,
}

pub fn enter(term: &mut Term<Listener>) {
    if !term.mode().contains(TermMode::VI) {
        term.toggle_vi_mode();
    }
}

pub fn leave(term: &mut Term<Listener>) {
    if term.mode().contains(TermMode::VI) {
        term.toggle_vi_mode();
    }
    term.selection = None;
    term.scroll_display(Scroll::Bottom);
}

pub fn take_key(copy: &mut CopyMode, term: &mut Term<Listener>, key: Key) -> Outcome {
    if copy.search.is_some() {
        return search_key(copy, term, key);
    }
    let name = key.to_string();
    let motion = match name.as_str() {
        "C-n" | "<down>" => Some(ViMotion::Down),
        "C-p" | "<up>" => Some(ViMotion::Up),
        "C-f" | "<right>" => Some(ViMotion::Right),
        "C-b" | "<left>" => Some(ViMotion::Left),
        "M-f" => Some(ViMotion::SemanticRightEnd),
        "M-b" => Some(ViMotion::SemanticLeft),
        "C-a" | "<home>" => Some(ViMotion::First),
        "C-e" | "<end>" => Some(ViMotion::Last),
        "M-{" => Some(ViMotion::ParagraphUp),
        "M-}" => Some(ViMotion::ParagraphDown),
        _ => None,
    };
    if let Some(motion) = motion {
        term.vi_motion(motion);
        let point = term.vi_mode_cursor.point;
        term.scroll_to_point(point);
        return Outcome::Handled;
    }
    match name.as_str() {
        "M-<" => goto(term, Point::new(term.topmost_line(), Column(0))),
        "M->" => goto(term, Point::new(term.bottommost_line(), Column(0))),
        "C-v" | "<next>" => page(term, 1),
        "M-v" | "<prior>" => page(term, -1),
        "C-SPC" => {
            let mut selection =
                Selection::new(SelectionType::Simple, term.vi_mode_cursor.point, Side::Left);
            selection.include_all();
            term.selection = Some(selection);
        }
        "C-s" | "C-r" => {
            copy.search = Some(Search {
                input: String::new(),
                forward: name == "C-s",
                matches: Vec::new(),
                current: None,
                origin: term.vi_mode_cursor.point,
            });
        }
        "M-w" | "s-c" => {
            return match term.selection_to_string() {
                Some(text) if !text.is_empty() => Outcome::Copy(text),
                _ => Outcome::Handled,
            };
        }
        "C-g" if term.selection.is_some() => term.selection = None,
        "q" | "C-g" | "ESC" => return Outcome::Leave,
        _ => return Outcome::Ignored,
    }
    Outcome::Handled
}

fn search_key(copy: &mut CopyMode, term: &mut Term<Listener>, key: Key) -> Outcome {
    let search = copy.search.as_mut().expect("searching");
    match (key.printable_char(), key.to_string().as_str()) {
        (Some(character), _) => {
            search.input.push(character);
            find(search, term, false);
        }
        (None, "DEL") => {
            search.input.pop();
            find(search, term, false);
        }
        (None, "C-s") => {
            search.forward = true;
            find(search, term, true);
        }
        (None, "C-r") => {
            search.forward = false;
            find(search, term, true);
        }
        (None, "RET") => copy.search = None,
        (None, "C-g") => {
            let origin = search.origin;
            copy.search = None;
            goto(term, origin);
        }
        _ if key.code == KeyCode::Escape => copy.search = None,
        _ => return Outcome::Ignored,
    }
    Outcome::Handled
}

/// Finds the input again from where the search started, or goes on to the next match
/// with `again`; matches are case-insensitive while the input is lower case, as in Emacs.
fn find(search: &mut Search, term: &mut Term<Listener>, again: bool) {
    if !again || search.matches.is_empty() {
        search.matches = all_matches(term, &search.input);
        search.current = None;
    }
    if search.matches.is_empty() {
        return;
    }
    let from = match search.current {
        Some(current) if again => current,
        _ => {
            let origin = search.origin;
            let first_after = search.matches.iter().position(|found| *found.start() >= origin);
            let index = if search.forward {
                first_after.unwrap_or(0)
            } else {
                first_after
                    .unwrap_or(search.matches.len())
                    .checked_sub(1)
                    .unwrap_or(search.matches.len() - 1)
            };
            search.current = Some(index);
            goto(term, *search.matches[index].start());
            return;
        }
    };
    let count = search.matches.len();
    let index = if search.forward { (from + 1) % count } else { (from + count - 1) % count };
    search.current = Some(index);
    goto(term, *search.matches[index].start());
}

fn all_matches(term: &Term<Listener>, input: &str) -> Vec<RangeInclusive<Point>> {
    if input.is_empty() {
        return Vec::new();
    }
    let escaped = regex_escape(input);
    let pattern =
        if input.chars().any(char::is_uppercase) { escaped } else { format!("(?i){escaped}") };
    let Ok(mut regex) = RegexSearch::new(&pattern) else {
        return Vec::new();
    };
    let start = Point::new(term.topmost_line(), Column(0));
    let end = Point::new(term.bottommost_line(), term.last_column());
    RegexIter::new(start, end, Direction::Right, term, &mut regex).collect()
}

fn regex_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn goto(term: &mut Term<Listener>, point: Point) {
    term.vi_goto_point(point);
}

fn page(term: &mut Term<Listener>, direction: i32) {
    let rows = term.screen_lines() as i32 - 2;
    let line = (term.vi_mode_cursor.point.line.0 + direction * rows)
        .clamp(term.topmost_line().0, term.bottommost_line().0);
    let column = term.vi_mode_cursor.point.column;
    goto(term, Point::new(Line(line), column));
}

/// `COPY`, and where the cursor is in the scrollback.
pub fn mode(term: &Term<Listener>) -> ScreenMode {
    let total = term.topmost_line().0.unsigned_abs() as usize + term.screen_lines();
    let line = (term.vi_mode_cursor.point.line.0 - term.topmost_line().0) as usize + 1;
    ScreenMode {
        label: "COPY",
        detail: format!("line {} of {}", thousands(line), thousands(total)),
        leave: "q",
    }
}

pub fn search_bar(search: &Search) -> ScreenSearch {
    ScreenSearch {
        input: search.input.clone(),
        found: search.current.map(|current| (current + 1, search.matches.len())),
        hints: SEARCH_HINTS,
    }
}

fn thousands(number: usize) -> String {
    let digits = number.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_thousands() {
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(1214), "1,214");
        assert_eq!(thousands(1_000_000), "1,000,000");
    }
}
