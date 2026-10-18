//! Built-in plugin: Emacs motion, editing, undo, the mark and the kill ring with their
//! default keys.

mod comment;
mod editing;
mod expand;
mod indent;
mod killing;
mod motion;
mod pairs;

use remacs_core::{Command, Editor, MenuItem, Plugin, Registry, SELF_INSERT_COMMAND};

pub struct EditingPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new("forward-char", "Move point one character forward", motion::forward_char),
    Command::new("backward-char", "Move point one character backward", motion::backward_char),
    Command::new("forward-word", "Move point over the next run of letters and digits or of other characters", motion::forward_word),
    Command::new("backward-word", "Move point back over the previous run of letters and digits or of other characters", motion::backward_word),
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
    Command::new("keyboard-quit", "Cancel the current command and deactivate the mark", editing::keyboard_quit),
    Command::new(killing::SET_MARK_COMMAND, "Set the mark at point and activate it", killing::set_mark_command),
    Command::new(killing::POP_TO_MARK_COMMAND, "Go to the mark, then, with C-SPC, to the marks before it", killing::pop_to_mark_command),
    Command::new("exchange-point-and-mark", "Swap point and mark, activating the region", killing::exchange_point_and_mark),
    Command::new("mark-whole-buffer", "Put point at the beginning and mark at the end", killing::mark_whole_buffer),
    Command::new("mark-paragraph", "Select the paragraph at point, or the next one", killing::mark_paragraph),
    Command::new("kill-region", "Kill the text between point and mark", killing::kill_region),
    Command::new("kill-ring-save", "Save the region as if killed, without deleting it", killing::kill_ring_save),
    Command::new("kill-line", "Kill the rest of the line", killing::kill_line),
    Command::new("kill-word", "Kill the next run of letters and digits or of other characters", killing::kill_word),
    Command::new("backward-kill-word", "Kill the previous run of letters and digits or of other characters", killing::backward_kill_word),
    Command::new("kill-paragraph", "Kill to the end of the paragraph", killing::kill_paragraph),
    Command::new(expand::EXPAND_REGION, "Select inside the enclosing brackets or quotes, then with them, then the next pair out", expand::expand_region),
    Command::new(expand::CONTRACT_REGION, "Go back to the selection the last expand-region grew from", expand::contract_region),
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
    ("M-}", "forward-paragraph"),
    ("M-{", "backward-paragraph"),
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
    MenuItem::new("Edit", "code", "Comment Line", "comment-line"),
    MenuItem::new("Go", "buffer", "Beginning of Buffer", "beginning-of-buffer"),
    MenuItem::new("Go", "buffer", "End of Buffer", "end-of-buffer"),
    MenuItem::new("Go", "buffer", "Previous Paragraph", "backward-paragraph"),
    MenuItem::new("Go", "buffer", "Next Paragraph", "forward-paragraph"),
];

impl Plugin for EditingPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry.menu_items.extend_from_slice(MENU_ITEMS);
    }
}

fn has_region(editor: &Editor) -> bool {
    editor.buffer().region().is_some()
}

fn after_yank(editor: &Editor) -> bool {
    matches!(
        editor.last_command(),
        Some(killing::YANK | killing::YANK_POP)
    )
}

#[cfg(test)]
mod tests;
