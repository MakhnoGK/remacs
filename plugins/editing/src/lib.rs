//! Built-in plugin: Emacs motion, editing, undo and file commands with their default keys.

mod editing;
mod files;
mod motion;

use remacs_core::{Command, Plugin, Registry, SELF_INSERT_COMMAND};

pub struct EditingPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new("forward-char", "Move point one character forward", motion::forward_char),
    Command::new("backward-char", "Move point one character backward", motion::backward_char),
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
    Command::new("newline", "Break the line, keeping its indentation", editing::newline),
    Command::new("tab-to-tab-stop", "Insert spaces up to the next tab stop", editing::tab_to_tab_stop),
    Command::new("delete-char", "Delete the character after point", editing::delete_char),
    Command::new("delete-backward-char", "Delete the character before point", editing::delete_backward_char),
    Command::new("undo", "Undo the last change", editing::undo),
    Command::new("undo-redo", "Redo the last undone change", editing::undo_redo),
    Command::new("keyboard-quit", "Cancel the current command", editing::keyboard_quit),
    Command::new("save-buffer", "Save the buffer to its file", files::save_buffer),
    Command::new(files::SAVE_BUFFERS_KILL_TERMINAL, "Quit remacs", files::save_buffers_kill_terminal),
];

const BINDINGS: &[(&str, &str)] = &[
    ("C-f", "forward-char"),
    ("<right>", "forward-char"),
    ("C-b", "backward-char"),
    ("<left>", "backward-char"),
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
    ("C-x C-s", "save-buffer"),
    ("s-s", "save-buffer"),
    ("C-x C-c", files::SAVE_BUFFERS_KILL_TERMINAL),
    ("s-q", files::SAVE_BUFFERS_KILL_TERMINAL),
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
