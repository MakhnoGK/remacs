//! Built-in plugin: Emacs motion, editing, undo, the mark and the kill ring with their
//! default keys.

mod comment;
mod cursors;
mod editing;
mod expand;
mod indent;
mod killing;
mod lines;
mod motion;
mod pairs;
mod rectangle;
mod transform;
mod transpose;

use std::rc::Rc;

use remacs_core::{Command, Editor, MenuItem, Plugin, Registry, SELF_INSERT_COMMAND, Section};
use serde::Deserialize;

pub struct EditingPlugin;

/// `editing.toml`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct EditingSettings {
    /// Cursors a buffer has at most, point included.
    pub max_cursors: usize,
}

impl Default for EditingSettings {
    fn default() -> Self {
        Self { max_cursors: 1000 }
    }
}

impl Section for EditingSettings {
    const NAME: &str = "editing";

    fn check(&self) -> Result<(), String> {
        match self.max_cursors {
            0 => Err("max-cursors is 0".into()),
            _ => Ok(()),
        }
    }
}

/// Commands that run again at each cursor, as multiple-cursors runs all but those it lists
/// to run once; those that read keys or text after them, or keep state between runs, do
/// not.
const EACH_CURSOR: &[&str] = &[
    "forward-char",
    "backward-char",
    "forward-word",
    "backward-word",
    "forward-paragraph",
    "backward-paragraph",
    "next-line",
    "previous-line",
    "move-beginning-of-line",
    "move-end-of-line",
    SELF_INSERT_COMMAND,
    "newline",
    "open-line-below",
    "open-line-above",
    "comment-line",
    "tab-to-tab-stop",
    "delete-char",
    "delete-backward-char",
    killing::SET_MARK_COMMAND,
    "exchange-point-and-mark",
    "mark-paragraph",
    "kill-region",
    "kill-ring-save",
    "kill-line",
    "kill-word",
    "backward-kill-word",
    "kill-paragraph",
    killing::YANK,
    "transpose-chars",
    "transpose-words",
    "transpose-lines",
    "join-line",
    "upcase-dwim",
    "downcase-dwim",
    "capitalize-dwim",
    "duplicate-dwim",
];

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new("forward-char", "Move point one character forward", motion::forward_char),
    Command::new("backward-char", "Move point one character backward", motion::backward_char),
    Command::new("forward-word", "Move point to the end of the next word or token, or to the next line", motion::forward_word),
    Command::new("backward-word", "Move point to the start of the previous word or token, or to the previous line", motion::backward_word),
    Command::new("forward-paragraph", "Move to the blank line after the paragraph", motion::forward_paragraph),
    Command::new("backward-paragraph", "Move to the blank line before the paragraph", motion::backward_paragraph),
    Command::new("next-line", "Move to the next line, keeping the column", motion::next_line),
    Command::new("previous-line", "Move to the previous line, keeping the column", motion::previous_line),
    Command::new("move-beginning-of-line", "Move to the beginning of the line", motion::move_beginning_of_line),
    Command::new("move-end-of-line", "Move to the end of the line", motion::move_end_of_line),
    Command::new("beginning-of-buffer", "Move to the beginning of the buffer", motion::beginning_of_buffer),
    Command::new("end-of-buffer", "Move to the end of the buffer", motion::end_of_buffer),
    Command::new("scroll-up-command", "Scroll forward by a screen", motion::scroll_up_command),
    Command::new("scroll-down-command", "Scroll backward by a screen", motion::scroll_down_command),
    Command::new("recenter", "Center the line with point in the window", motion::recenter),
    Command::new(motion::RECENTER_TOP_BOTTOM, "Center the line with point, then put it at the top, then at the bottom", motion::recenter_top_bottom),
    Command::new(motion::MOVE_TO_WINDOW_LINE_TOP_BOTTOM, "Move point to the middle line of the window, then the top, then the bottom", motion::move_to_window_line_top_bottom),
    Command::new(SELF_INSERT_COMMAND, "Insert the character typed", editing::self_insert),
    Command::new("newline", "Break the line and indent the new one", indent::newline),
    Command::new("open-line-below", "Start a new line below, leaving this one whole", indent::open_line_below),
    Command::new("open-line-above", "Start a new line above, leaving this one whole", indent::open_line_above),
    Command::new("comment-line", "Comment out the line or region, or uncomment it", comment::comment_line),
    Command::new("tab-to-tab-stop", "Indent to the next tab stop", indent::tab_to_tab_stop),
    Command::new("delete-char", "Delete the next character or the region", editing::delete_char),
    Command::new("delete-backward-char", "Delete the previous character or the region", editing::delete_backward_char),
    Command::new("undo", "Undo the last change", editing::undo),
    Command::new("undo-redo", "Redo the last undone change", editing::undo_redo),
    Command::new("keyboard-quit", "Cancel the current command and deactivate the mark, or remove the cursors", cursors::keyboard_quit),
    Command::new(killing::SET_MARK_COMMAND, "Set the mark at point and activate it", killing::set_mark_command),
    Command::new(killing::POP_TO_MARK_COMMAND, "Go to the mark, then, with C-SPC, to the marks before it", killing::pop_to_mark_command),
    Command::new("exchange-point-and-mark", "Swap point and mark, activating the region", killing::exchange_point_and_mark),
    Command::new("mark-whole-buffer", "Put point at the beginning and mark at the end", killing::mark_whole_buffer),
    Command::new("mark-paragraph", "Select the paragraph at point, or the next one", killing::mark_paragraph),
    Command::new("kill-region", "Kill the text between point and mark", killing::kill_region),
    Command::new("kill-ring-save", "Save the region as if killed, without deleting it", killing::kill_ring_save),
    Command::new("kill-line", "Kill the rest of the line", killing::kill_line),
    Command::new("kill-word", "Kill to the next start or end of a word or token, within the line", killing::kill_word),
    Command::new("backward-kill-word", "Kill back to the start of the previous word or token, within the line", killing::backward_kill_word),
    Command::new("kill-paragraph", "Kill to the end of the paragraph", killing::kill_paragraph),
    Command::new(expand::EXPAND_REGION, "Select inside the enclosing brackets or quotes, then with them, then the next pair out", expand::expand_region),
    Command::new(expand::CONTRACT_REGION, "Go back to the selection the last expand-region grew from", expand::contract_region),
    Command::new(killing::YANK, "Insert the last killed text", killing::yank),
    Command::new(killing::YANK_POP, "Replace the yanked text with an earlier kill", killing::yank_pop),
    Command::new("transpose-chars", "Swap the characters around point, or the two before it at the end of a line", transpose::transpose_chars),
    Command::new("transpose-words", "Swap the words around point", transpose::transpose_words),
    Command::new("transpose-lines", "Swap the line with the one above", transpose::transpose_lines),
    Command::new(transform::ZAP_TO_CHAR, "Kill up to and with the next occurrence of a character", transform::zap_to_char),
    Command::new(transform::ZAP_UP_TO_CHAR, "Kill up to the next occurrence of a character", transform::zap_up_to_char),
    Command::new("join-line", "Join the line to the one above, or the region's lines into one", transform::join_line),
    Command::new(transform::CYCLE_SPACING, "Leave one space around point, then none, then what was there", transform::cycle_spacing),
    Command::new("upcase-dwim", "Upcase the region, or to the end of the word", transform::upcase_dwim),
    Command::new("downcase-dwim", "Downcase the region, or to the end of the word", transform::downcase_dwim),
    Command::new("capitalize-dwim", "Capitalize the region, or to the end of the word", transform::capitalize_dwim),
    Command::new("duplicate-dwim", "Copy the line under it, or the region after it, and go to the copy", transform::duplicate_dwim),
    Command::new("move-line-up", "Move the line, or the region's lines, above the line before", transform::move_line_up),
    Command::new("move-line-down", "Move the line, or the region's lines, below the line after", transform::move_line_down),
    Command::new("sort-lines", "Sort the region's lines, or the buffer's", lines::sort_lines),
    Command::new("delete-duplicate-lines", "Delete repeats of the region's lines, or the buffer's", lines::delete_duplicate_lines),
    Command::new("keep-lines", "Delete the region's lines, or the buffer's, that do not match a regexp", lines::keep_lines),
    Command::new("flush-lines", "Delete the region's lines, or the buffer's, that match a regexp", lines::flush_lines),
    Command::new("align-regexp", "Line up the first match of a regexp on the region's lines", lines::align_regexp),
    Command::new("align-regexp-repeat", "Line up every match of a regexp on the region's lines, in columns", lines::align_regexp_repeat),
    Command::new(cursors::MARK_NEXT, "Add a cursor at the next match of the region, or on the next line", cursors::mark_next_like_this),
    Command::new(cursors::MARK_PREVIOUS, "Add a cursor at the previous match of the region, or on the line above", cursors::mark_previous_like_this),
    Command::new(cursors::SKIP_TO_NEXT, "Move the last cursor added to the next match of the region", cursors::skip_to_next_like_this),
    Command::new(cursors::MARK_ALL, "Add a cursor at every match of the region", cursors::mark_all_like_this),
    Command::new(cursors::EDIT_LINES, "Add a cursor on each line of the region", cursors::edit_lines),
    Command::new(cursors::IEDIT_MODE, "Add a cursor at each occurrence of the symbol at point or of the region, or remove the cursors", cursors::iedit_mode),
    Command::new(cursors::REMOVE_CURSORS, "Remove the cursors but point", cursors::remove_cursors),
    Command::new("rectangle-mark-mode", "Make the region a rectangle, or a run of text again", rectangle::rectangle_mark_mode),
    Command::new("string-rectangle", "Replace each line of the rectangle with a string", rectangle::string_rectangle),
    Command::new("kill-rectangle", "Kill the rectangle, for yank-rectangle", rectangle::kill_rectangle),
    Command::new("copy-rectangle-as-kill", "Copy the rectangle, for yank-rectangle", rectangle::copy_rectangle_as_kill),
    Command::new("yank-rectangle", "Insert the last rectangle killed, its corner at point", rectangle::yank_rectangle),
    Command::new("delete-rectangle", "Delete the rectangle", rectangle::delete_rectangle),
    Command::new("open-rectangle", "Insert blank space over the rectangle, moving its text right", rectangle::open_rectangle),
];

const BINDINGS: &[(&str, &str)] = &[
    ("C-f", "forward-char"),
    ("<right>", "forward-char"),
    ("C-b", "backward-char"),
    ("<left>", "backward-char"),
    ("M-f", "forward-word"),
    ("M-<right>", "forward-word"),
    ("M-b", "backward-word"),
    ("M-<left>", "backward-word"),
    ("M-}", "forward-paragraph"),
    ("M-{", "backward-paragraph"),
    ("C-n", "next-line"),
    ("<down>", "next-line"),
    ("C-p", "previous-line"),
    ("<up>", "previous-line"),
    ("C-a", "move-beginning-of-line"),
    ("<home>", "move-beginning-of-line"),
    ("s-<left>", "move-beginning-of-line"),
    ("C-e", "move-end-of-line"),
    ("<end>", "move-end-of-line"),
    ("s-<right>", "move-end-of-line"),
    ("M-<", "beginning-of-buffer"),
    ("s-<up>", "beginning-of-buffer"),
    ("M->", "end-of-buffer"),
    ("s-<down>", "end-of-buffer"),
    ("C-v", "scroll-up-command"),
    ("<next>", "scroll-up-command"),
    ("M-v", "scroll-down-command"),
    ("<prior>", "scroll-down-command"),
    ("C-l", motion::RECENTER_TOP_BOTTOM),
    ("M-r", motion::MOVE_TO_WINDOW_LINE_TOP_BOTTOM),
    ("RET", "newline"),
    ("s-RET", "open-line-below"),
    ("s-S-RET", "open-line-above"),
    ("M-;", "comment-line"),
    ("s-/", "comment-line"),
    ("TAB", "tab-to-tab-stop"),
    ("C-d", "delete-char"),
    ("<deletechar>", "delete-char"),
    ("DEL", "delete-backward-char"),
    ("C-/", "undo"),
    ("C-_", "undo"),
    ("C-x u", "undo"),
    ("s-z", "undo"),
    ("C-?", "undo-redo"),
    ("s-Z", "undo-redo"),
    ("C-g", "keyboard-quit"),
    ("C-SPC", killing::SET_MARK_COMMAND),
    ("C-u C-SPC", killing::POP_TO_MARK_COMMAND),
    ("C-x C-x", "exchange-point-and-mark"),
    ("s-j", "exchange-point-and-mark"),
    ("C-x h", "mark-whole-buffer"),
    ("s-a", "mark-whole-buffer"),
    ("M-h", "mark-paragraph"),
    ("C-w", "kill-region"),
    ("s-x", "kill-region"),
    ("M-w", "kill-ring-save"),
    ("s-c", "kill-ring-save"),
    ("C-k", "kill-line"),
    ("M-d", "kill-word"),
    ("M-DEL", "backward-kill-word"),
    ("C-=", expand::EXPAND_REGION),
    ("M-<up>", expand::EXPAND_REGION),
    ("C--", expand::CONTRACT_REGION),
    ("M-<down>", expand::CONTRACT_REGION),
    ("C-y", killing::YANK),
    ("s-v", killing::YANK),
    ("M-y", killing::YANK_POP),
    ("C-t", "transpose-chars"),
    ("M-t", "transpose-words"),
    ("C-x C-t", "transpose-lines"),
    ("M-z", transform::ZAP_TO_CHAR),
    ("M-Z", transform::ZAP_UP_TO_CHAR),
    ("M-^", "join-line"),
    ("M-SPC", transform::CYCLE_SPACING),
    ("M-u", "upcase-dwim"),
    ("M-l", "downcase-dwim"),
    ("M-c", "capitalize-dwim"),
    ("s-d", "duplicate-dwim"),
    ("M-S-<up>", "move-line-up"),
    ("M-S-<down>", "move-line-down"),
    ("C-x \\", "align-regexp"),
    ("C-x |", "align-regexp-repeat"),
    ("C->", cursors::MARK_NEXT),
    ("C-<", cursors::MARK_PREVIOUS),
    ("C-c C->", cursors::SKIP_TO_NEXT),
    ("C-c C-<", cursors::MARK_ALL),
    ("C-S-c C-S-c", cursors::EDIT_LINES),
    ("C-;", cursors::IEDIT_MODE),
    ("C-x SPC", "rectangle-mark-mode"),
    ("C-x r t", "string-rectangle"),
    ("C-x r k", "kill-rectangle"),
    ("C-x r M-w", "copy-rectangle-as-kill"),
    ("C-x r y", "yank-rectangle"),
    ("C-x r d", "delete-rectangle"),
    ("C-x r o", "open-rectangle"),
];

#[rustfmt::skip]
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::new("Edit", "undo", "Undo", "undo"),
    MenuItem::new("Edit", "undo", "Redo", "undo-redo"),
    MenuItem::new("Edit", "clipboard", "Cut", "kill-region").enabled_when(has_region),
    MenuItem::new("Edit", "clipboard", "Copy", "kill-ring-save").enabled_when(has_region),
    MenuItem::new("Edit", "clipboard", "Paste", killing::YANK),
    MenuItem::new("Edit", "clipboard", "Paste Earlier", killing::YANK_POP).enabled_when(after_yank),
    MenuItem::new("Edit", "selection", "Select All", "mark-whole-buffer"),
    MenuItem::new("Edit", "selection", "Select Paragraph", "mark-paragraph"),
    MenuItem::new("Edit", "selection", "Set Mark", killing::SET_MARK_COMMAND),
    MenuItem::new("Edit", "selection", "Expand Selection", expand::EXPAND_REGION),
    MenuItem::new("Edit", "selection", "Shrink Selection", expand::CONTRACT_REGION).enabled_when(has_region),
    MenuItem::new("Edit", "cursors", "Add Cursor at Next Match", cursors::MARK_NEXT),
    MenuItem::new("Edit", "cursors", "Add Cursor at Previous Match", cursors::MARK_PREVIOUS),
    MenuItem::new("Edit", "cursors", "Skip to Next Match", cursors::SKIP_TO_NEXT).enabled_when(has_region),
    MenuItem::new("Edit", "cursors", "Add Cursors at All Matches", cursors::MARK_ALL).enabled_when(has_region),
    MenuItem::new("Edit", "cursors", "Add Cursors to Lines", cursors::EDIT_LINES).enabled_when(has_region),
    MenuItem::new("Edit", "cursors", "Edit All Occurrences", cursors::IEDIT_MODE),
    MenuItem::new("Edit", "cursors", "Remove Cursors", cursors::REMOVE_CURSORS).enabled_when(cursors::has_cursors),
    MenuItem::new("Edit", "code", "Comment Line", "comment-line"),
    MenuItem::new("Edit", "lines", "Duplicate", "duplicate-dwim"),
    MenuItem::new("Edit", "lines", "Move Line Up", "move-line-up"),
    MenuItem::new("Edit", "lines", "Move Line Down", "move-line-down"),
    MenuItem::new("Edit", "lines", "Join Lines", "join-line"),
    MenuItem::new("Edit", "lines", "Sort Lines", "sort-lines"),
    MenuItem::new("Edit", "lines", "Delete Duplicate Lines", "delete-duplicate-lines"),
    MenuItem::new("Edit", "lines", "Keep Lines Matching…", "keep-lines"),
    MenuItem::new("Edit", "lines", "Delete Lines Matching…", "flush-lines"),
    MenuItem::new("Edit", "lines", "Align…", "align-regexp").enabled_when(has_region),
    MenuItem::new("Edit", "lines", "Align Every Match…", "align-regexp-repeat").enabled_when(has_region),
    MenuItem::new("Edit", "transform", "Upper Case", "upcase-dwim"),
    MenuItem::new("Edit", "transform", "Lower Case", "downcase-dwim"),
    MenuItem::new("Edit", "transform", "Capitalize", "capitalize-dwim"),
    MenuItem::new("Edit", "transform", "Transpose Characters", "transpose-chars"),
    MenuItem::new("Edit", "transform", "Transpose Words", "transpose-words"),
    MenuItem::new("Edit", "transform", "Transpose Lines", "transpose-lines"),
    MenuItem::new("Edit", "rectangle", "Rectangle Selection", "rectangle-mark-mode"),
    MenuItem::new("Edit", "rectangle", "Replace Rectangle…", "string-rectangle").enabled_when(has_mark),
    MenuItem::new("Edit", "rectangle", "Cut Rectangle", "kill-rectangle").enabled_when(has_mark),
    MenuItem::new("Edit", "rectangle", "Copy Rectangle", "copy-rectangle-as-kill").enabled_when(has_mark),
    MenuItem::new("Edit", "rectangle", "Paste Rectangle", "yank-rectangle"),
    MenuItem::new("Edit", "rectangle", "Delete Rectangle", "delete-rectangle").enabled_when(has_mark),
    MenuItem::new("Edit", "rectangle", "Open Rectangle", "open-rectangle").enabled_when(has_mark),
    MenuItem::new("Go", "buffer", "Beginning of Buffer", "beginning-of-buffer"),
    MenuItem::new("Go", "buffer", "End of Buffer", "end-of-buffer"),
    MenuItem::new("Go", "buffer", "Previous Paragraph", "backward-paragraph"),
    MenuItem::new("Go", "buffer", "Next Paragraph", "forward-paragraph"),
];

impl Plugin for EditingPlugin {
    fn register(&self, registry: &mut Registry) {
        for &command in COMMANDS {
            registry
                .commands
                .add(if EACH_CURSOR.contains(&command.name) {
                    command.each_cursor()
                } else {
                    command
                });
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry.menu_items.extend_from_slice(MENU_ITEMS);
        registry.key_sinks.push(Rc::new(transform::take_zap_key));
        registry.settings_hooks.push(Rc::new(|editor| {
            let settings = editor.settings_section::<EditingSettings>();
            *editor.plugin_state() = settings;
        }));
        registry.status_providers.push(Rc::new(cursors::status));
    }
}

fn has_region(editor: &Editor) -> bool {
    editor.buffer().region().is_some()
}

fn has_mark(editor: &Editor) -> bool {
    editor.buffer().mark().is_some()
}

fn after_yank(editor: &Editor) -> bool {
    matches!(
        editor.last_command(),
        Some(killing::YANK | killing::YANK_POP)
    )
}

#[cfg(test)]
mod tests;
