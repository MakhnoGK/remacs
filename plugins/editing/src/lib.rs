//! Built-in plugin: Emacs motion, editing, undo, the mark and the kill ring with their
//! default keys.

mod editing;
mod indent;
mod killing;
mod motion;
mod pairs;

use remacs_core::{Command, Plugin, Registry, SELF_INSERT_COMMAND};

pub struct EditingPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new("forward-char", "Move point one character forward", motion::forward_char),
    Command::new("backward-char", "Move point one character backward", motion::backward_char),
    Command::new("forward-word", "Move point to the end of the next word", motion::forward_word),
    Command::new("backward-word", "Move point to the start of the previous word", motion::backward_word),
    Command::new("next-line", "Move to the next line, keeping the column", motion::next_line),
    Command::new("previous-line", "Move to the previous line, keeping the column", motion::previous_line),
    Command::new("move-beginning-of-line", "Move to the beginning of the line", motion::move_beginning_of_line),
    Command::new("move-end-of-line", "Move to the end of the line", motion::move_end_of_line),
    Command::new("beginning-of-buffer", "Move to the beginning of the buffer", motion::beginning_of_buffer),
    Command::new("end-of-buffer", "Move to the end of the buffer", motion::end_of_buffer),
    Command::new("scroll-up-command", "Scroll forward by a screen", motion::scroll_up_command),
    Command::new("scroll-down-command", "Scroll backward by a screen", motion::scroll_down_command),
    Command::new("recenter", "Center the line with point in the window", motion::recenter),
    Command::new(SELF_INSERT_COMMAND, "Insert the character typed", editing::self_insert),
    Command::new("newline", "Break the line and indent the new one", indent::newline),
    Command::new("open-line-below", "Start a new line below, leaving this one whole", indent::open_line_below),
    Command::new("open-line-above", "Start a new line above, leaving this one whole", indent::open_line_above),
    Command::new("tab-to-tab-stop", "Indent to the next tab stop", indent::tab_to_tab_stop),
    Command::new("delete-char", "Delete the next character or the region", editing::delete_char),
    Command::new("delete-backward-char", "Delete the previous character or the region", editing::delete_backward_char),
    Command::new("undo", "Undo the last change", editing::undo),
    Command::new("undo-redo", "Redo the last undone change", editing::undo_redo),
    Command::new("keyboard-quit", "Cancel the current command and deactivate the mark", editing::keyboard_quit),
    Command::new(killing::SET_MARK_COMMAND, "Set the mark at point and activate it", killing::set_mark_command),
    Command::new("exchange-point-and-mark", "Swap point and mark, activating the region", killing::exchange_point_and_mark),
    Command::new("mark-whole-buffer", "Put point at the beginning and mark at the end", killing::mark_whole_buffer),
    Command::new("kill-region", "Kill the text between point and mark", killing::kill_region),
    Command::new("kill-ring-save", "Save the region as if killed, without deleting it", killing::kill_ring_save),
    Command::new("kill-line", "Kill the rest of the line", killing::kill_line),
    Command::new("kill-word", "Kill up to the end of the next word", killing::kill_word),
    Command::new("backward-kill-word", "Kill back to the start of the previous word", killing::backward_kill_word),
    Command::new(killing::YANK, "Insert the last killed text", killing::yank),
    Command::new(killing::YANK_POP, "Replace the yanked text with an earlier kill", killing::yank_pop),
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
    ("C-n", "next-line"),
    ("<down>", "next-line"),
    ("C-p", "previous-line"),
    ("<up>", "previous-line"),
    ("C-a", "move-beginning-of-line"),
    ("<home>", "move-beginning-of-line"),
    ("C-e", "move-end-of-line"),
    ("<end>", "move-end-of-line"),
    ("M-<", "beginning-of-buffer"),
    ("s-<up>", "beginning-of-buffer"),
    ("M->", "end-of-buffer"),
    ("s-<down>", "end-of-buffer"),
    ("C-v", "scroll-up-command"),
    ("<next>", "scroll-up-command"),
    ("M-v", "scroll-down-command"),
    ("<prior>", "scroll-down-command"),
    ("C-l", "recenter"),
    ("RET", "newline"),
    ("s-RET", "open-line-below"),
    ("s-S-RET", "open-line-above"),
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
    ("C-x C-x", "exchange-point-and-mark"),
    ("C-x h", "mark-whole-buffer"),
    ("s-a", "mark-whole-buffer"),
    ("C-w", "kill-region"),
    ("s-x", "kill-region"),
    ("M-w", "kill-ring-save"),
    ("s-c", "kill-ring-save"),
    ("C-k", "kill-line"),
    ("M-d", "kill-word"),
    ("M-DEL", "backward-kill-word"),
    ("C-y", killing::YANK),
    ("s-v", killing::YANK),
    ("M-y", killing::YANK_POP),
];

impl Plugin for EditingPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
    }
}

#[cfg(test)]
mod tests;
