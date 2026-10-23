//! Jumping to text on screen by the label drawn on it, like Emacs `avy`: type what to go
//! to, then the label of the place among those shown. Keys go to it before the keymaps
//! while it reads them.

use std::ops::Range;
use std::time::{Duration, Instant};

use remacs_core::{Buffer, CharClass, CommandResult, Editor, Key, TextLabel, describe_sequence};

use crate::NavigationSettings;

pub const AVY_GOTO_CHAR_TIMER: &str = "avy-goto-char-timer";
pub const AVY_GOTO_WORD: &str = "avy-goto-word-1";
pub const AVY_GOTO_LINE: &str = "avy-goto-line";
const AVY_COMMANDS: &[&str] = &[AVY_GOTO_CHAR_TIMER, AVY_GOTO_WORD, AVY_GOTO_LINE];

#[derive(Default)]
enum Avy {
    #[default]
    Idle,
    /// Reading what to jump to; for words a single character, done once typed.
    Reading {
        words: bool,
        typed: String,
        /// When typing stops counting as more of the text, once something is typed.
        done_at: Option<Instant>,
    },
    /// Reading the label of a place, `typed` its start so far.
    Choosing {
        places: Vec<(usize, String)>,
        typed: String,
    },
}

pub(crate) fn avy_goto_char_timer(editor: &mut Editor) -> CommandResult {
    start_reading(editor, false);
    Ok(())
}

pub(crate) fn avy_goto_word(editor: &mut Editor) -> CommandResult {
    start_reading(editor, true);
    Ok(())
}

pub(crate) fn avy_goto_line(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer();
    let text = buffer.text();
    let starts = shown_lines(editor)
        .filter_map(|line| text.line_start(line))
        .collect();
    choose(editor, starts);
    Ok(())
}

fn start_reading(editor: &mut Editor, words: bool) {
    *editor.plugin_state::<Avy>() = Avy::Reading {
        words,
        typed: String::new(),
        done_at: None,
    };
    editor.set_message(prompt(words, ""));
}

fn prompt(words: bool, typed: &str) -> String {
    let what = if words {
        "Word starting with"
    } else {
        "Jump to"
    };
    format!("{what}: {typed}")
}

/// Takes the keys while reading what to jump to or a label; any other key quits.
pub(crate) fn take_key(editor: &mut Editor, key: Key) -> bool {
    if matches!(editor.plugin_state::<Avy>(), Avy::Idle) {
        return false;
    }
    let state = std::mem::take(editor.plugin_state::<Avy>());
    let typed_char = key.printable_char();
    match (state, describe_sequence(&[key]).as_str()) {
        (_, "C-g" | "ESC") => editor.set_message("Quit"),
        (Avy::Reading { words, typed, .. }, "RET") => finish_reading(editor, words, &typed),
        (
            Avy::Reading {
                words, mut typed, ..
            },
            "DEL",
        ) => {
            typed.pop();
            editor.set_message(prompt(words, &typed));
            *editor.plugin_state() = Avy::Reading {
                words,
                typed,
                done_at: None,
            };
        }
        (Avy::Choosing { places, mut typed }, "DEL") => {
            typed.pop();
            *editor.plugin_state() = Avy::Choosing { places, typed };
        }
        (
            Avy::Reading {
                words, mut typed, ..
            },
            _,
        ) if typed_char.is_some() => {
            typed.extend(typed_char);
            if words {
                finish_reading(editor, true, &typed);
                return true;
            }
            let wait =
                Duration::from_millis(editor.plugin_state::<NavigationSettings>().avy_timeout);
            editor.set_message(prompt(false, &typed));
            *editor.plugin_state() = Avy::Reading {
                words,
                typed,
                done_at: Some(Instant::now() + wait),
            };
            editor.refresh_after(wait);
        }
        (Avy::Choosing { places, mut typed }, _) if typed_char.is_some() => {
            typed.extend(typed_char);
            let mut matching = places.iter().filter(|(_, label)| label.starts_with(&typed));
            match (matching.next(), matching.next()) {
                (None, _) => editor.set_error(format!("No label {typed}")),
                (Some((offset, label)), None) if *label == typed => jump(editor, *offset),
                _ => *editor.plugin_state() = Avy::Choosing { places, typed },
            }
        }
        _ => editor.set_message("Quit"),
    }
    true
}

/// Labels what was typed once typing has paused; run after every command and refresh.
/// Another command, as one from the menu, ends the reading.
pub(crate) fn update(editor: &mut Editor) {
    let other_command = editor
        .last_command()
        .is_none_or(|command| !AVY_COMMANDS.contains(&command));
    if other_command {
        *editor.plugin_state() = Avy::Idle;
        return;
    }
    let Avy::Reading {
        words: false,
        typed,
        done_at: Some(done_at),
    } = editor.plugin_state::<Avy>()
    else {
        return;
    };
    if Instant::now() < *done_at {
        return;
    }
    let typed = std::mem::take(typed);
    *editor.plugin_state() = Avy::Idle;
    finish_reading(editor, false, &typed);
}

/// Labels the places that match `typed` on screen, or goes to the only one; for words,
/// those starting with it. Matches ignore case unless `typed` has capitals.
fn finish_reading(editor: &mut Editor, words: bool, typed: &str) {
    *editor.plugin_state() = Avy::Idle;
    if typed.is_empty() {
        return editor.set_message("Quit");
    }
    let buffer = editor.window_buffer();
    let places: Vec<usize> = shown_parts(editor)
        .flat_map(|range| matches(buffer, range, typed, words))
        .collect();
    if places.is_empty() {
        return editor.set_error(format!("No {typed} on screen"));
    }
    choose(editor, places);
}

fn matches(buffer: &Buffer, range: Range<usize>, typed: &str, words: bool) -> Vec<usize> {
    let shown = buffer.contents(range.clone());
    let exact = typed.chars().any(char::is_uppercase);
    let fold = |text: &str| {
        if exact {
            text.to_owned()
        } else {
            text.to_lowercase()
        }
    };
    let (haystack, needle) = (fold(&shown), fold(typed));
    // Lowercasing may change lengths; such texts are matched as typed.
    let haystack = if haystack.len() == shown.len() {
        haystack
    } else {
        shown.clone()
    };
    haystack
        .match_indices(&needle)
        .map(|(index, _)| index)
        // Lowercasing kept the length but not where characters start, as in "İẞ".
        .filter(|&index| shown.is_char_boundary(index))
        .filter(|&index| {
            !words
                || shown[..index]
                    .chars()
                    .next_back()
                    .is_none_or(|before| CharClass::of(before) != CharClass::Word)
        })
        .map(|index| range.start + index)
        .collect()
}

/// Labels `places`, the nearest to point first, or goes to the only one.
fn choose(editor: &mut Editor, mut places: Vec<usize>) {
    match places.as_slice() {
        [] => return editor.set_error("Nothing on screen"),
        [offset] => return jump(editor, *offset),
        _ => {}
    }
    let point = editor.window_buffer().point();
    places.sort_by_key(|offset| offset.abs_diff(point));
    let keys: Vec<char> = editor
        .plugin_state::<NavigationSettings>()
        .avy_keys
        .chars()
        .collect();
    let labels = labels(&keys, places.len());
    *editor.plugin_state() = Avy::Choosing {
        places: places.into_iter().zip(labels).collect(),
        typed: String::new(),
    };
    editor.set_message("Label:");
}

/// `count` labels of `keys`, all as long as the most needs.
fn labels(keys: &[char], count: usize) -> Vec<String> {
    let mut length = 1;
    while keys.len().pow(length) < count {
        length += 1;
    }
    (0..count)
        .map(|mut index| {
            let mut label = Vec::with_capacity(length as usize);
            for _ in 0..length {
                label.push(keys[index % keys.len()]);
                index /= keys.len();
            }
            label.into_iter().rev().collect()
        })
        .collect()
}

/// Goes to `offset`, pushing the mark unless a region is being selected, as avy does.
fn jump(editor: &mut Editor, offset: usize) {
    *editor.plugin_state() = Avy::Idle;
    editor.clear_message();
    let buffer = editor.window_buffer_mut();
    if !buffer.is_mark_active() {
        buffer.push_mark();
    }
    buffer.set_point(offset);
}

/// The window buffer's lines on screen, but those folds hide.
fn shown_lines(editor: &Editor) -> impl Iterator<Item = usize> + use<> {
    let shown = editor.window_buffer().shown_lines();
    let first = shown.row_of(editor.viewport.first_line);
    let rows = first..(first + editor.viewport.height.max(1)).min(shown.rows());
    rows.map(move |row| shown.line_at(row))
}

/// The bytes of each line on screen across the window's columns, so that a line of
/// megabytes is searched only where it shows.
fn shown_parts(editor: &Editor) -> impl Iterator<Item = Range<usize>> + '_ {
    let text = editor.window_buffer().text();
    let viewport = editor.viewport;
    // shortcut: character columns stand for display columns, which tabs and wide
    // characters make larger; scrolled sideways, such a line is searched a little off.
    // 0 until the window is first laid out.
    let width = if viewport.width == 0 {
        usize::MAX
    } else {
        viewport.width
    };
    let columns = viewport.first_column..viewport.first_column.saturating_add(width);
    shown_lines(editor).map(move |line| {
        text.offset_at_char_column(line, columns.start)
            ..text.offset_at_char_column(line, columns.end)
    })
}

/// What is left to type of each label on `lines`.
pub(crate) fn text_labels(editor: &Editor, lines: Range<usize>) -> Vec<TextLabel> {
    let Some(Avy::Choosing { places, typed }) = editor.find_plugin_state::<Avy>() else {
        return Vec::new();
    };
    let text = editor.window_buffer().text();
    places
        .iter()
        .filter(|(offset, label)| {
            label.starts_with(typed.as_str()) && lines.contains(&text.line_of(*offset))
        })
        .map(|(offset, label)| TextLabel {
            offset: *offset,
            text: label[typed.len()..].to_owned(),
        })
        .collect()
}
