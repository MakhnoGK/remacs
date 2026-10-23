//! Built-in plugin: visiting and saving files, switching and killing buffers, windows,
//! quitting.

mod buffers;
mod encoding;
mod file_index;
mod find_file;
mod ignored;
pub mod links;
mod preview;
mod project;
mod revert;
mod saving;
mod session;
mod windows;

use std::rc::Rc;

use remacs_core::{Command, Editor, MenuItem, Plugin, Registry};

use crate::buffers::KillBufferPrompt;
use crate::file_index::FileIndexes;
use crate::ignored::FilesSettings;
use crate::revert::AutoRevert;

pub use file_index::FileIndex;
pub use find_file::{directory_candidates, project_files};
pub use preview::FilePreview;
pub use project::{abbreviate_home, default_directory, expand_home, project_root, with_slash};
pub use session::keep_session_in;
pub use windows::WindowsSettings;

pub struct FilesPlugin;

/// Asks before losing unsaved changes or running programs, then quits; every way out of
/// the app goes through it.
pub const QUIT: &str = "save-buffers-kill-terminal";

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new("find-file", "Visit a file of the project, or any file by path", find_file::find_file),
    Command::new("switch-to-buffer", "Show another buffer in the window", buffers::switch_to_buffer),
    Command::new("kill-buffer", "Kill a buffer, asking first if it is modified", buffers::kill_buffer),
    Command::new("kill-current-buffer", "Kill the buffer in the window, asking first if it is modified", buffers::kill_current_buffer),
    Command::new("other-window", "Give the keys to the next window, or to the pane under them", windows::other_window),
    Command::new("split-window-below", "Split the window into two, one above the other", windows::split_window_below),
    Command::new("split-window-right", "Split the window into two side by side", windows::split_window_right),
    Command::new("delete-window", "Remove the window, giving its room to the one beside it", windows::delete_window),
    Command::new("delete-other-windows", "Make the window fill the frame", windows::delete_other_windows),
    Command::new("switch-to-buffer-other-window", "Show another buffer in another window and select it", buffers::switch_to_buffer_other_window),
    Command::new("find-file-other-window", "Visit a file in another window and select it", find_file::find_file_other_window),
    Command::new("enlarge-window", "Make the window taller", windows::enlarge_window),
    Command::new("enlarge-window-horizontally", "Make the window wider", windows::enlarge_window_horizontally),
    Command::new("shrink-window-horizontally", "Make the window narrower", windows::shrink_window_horizontally),
    Command::new("balance-windows", "Give the windows equal room", windows::balance_windows),
    Command::new("save-buffer", "Save current buffer in its visited file", saving::save_buffer),
    Command::new("write-file", "Write current buffer into a new file and visit it", saving::write_file),
    Command::new("save-some-buffers", "Offer to save each modified buffer", saving::save_some_buffers),
    Command::new(QUIT, "Offer to save modified buffers, ask about running programs, then exit", saving::save_buffers_kill_terminal),
    Command::new("revert-buffer", "Read the file again, dropping unsaved changes after asking", revert::revert_buffer),
    Command::new("revert-buffer-with-coding-system", "Read the file again in an encoding chosen, and save it so", encoding::revert_buffer_with_coding_system),
];

const BINDINGS: &[(&str, &str)] = &[
    ("C-x C-f", "find-file"),
    ("s-o", "find-file"),
    ("C-x b", "switch-to-buffer"),
    ("C-x k", "kill-buffer"),
    ("s-k", "kill-current-buffer"),
    ("s-u", "revert-buffer"),
    ("C-x o", "other-window"),
    ("C-x 2", "split-window-below"),
    ("C-x 3", "split-window-right"),
    ("C-x 0", "delete-window"),
    ("C-x 1", "delete-other-windows"),
    ("C-x 4 b", "switch-to-buffer-other-window"),
    ("C-x 4 f", "find-file-other-window"),
    ("C-x ^", "enlarge-window"),
    ("C-x }", "enlarge-window-horizontally"),
    ("C-x {", "shrink-window-horizontally"),
    ("C-x +", "balance-windows"),
    ("C-x C-s", "save-buffer"),
    ("s-s", "save-buffer"),
    ("C-x C-w", "write-file"),
    ("s-S", "write-file"),
    ("C-x s", "save-some-buffers"),
    ("C-x C-c", QUIT),
    ("s-q", QUIT),
];

#[rustfmt::skip]
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::new("File", "open", "Open…", "find-file"),
    MenuItem::new("File", "open", "Switch to Buffer…", "switch-to-buffer"),
    MenuItem::new("File", "open", "Revert to Saved", "revert-buffer")
        .enabled_when(encoding::visits_file),
    MenuItem::new("File", "open", "Reopen with Encoding…", "revert-buffer-with-coding-system")
        .enabled_when(encoding::visits_file),
    MenuItem::new("File", "save", "Save", "save-buffer"),
    MenuItem::new("File", "save", "Save As…", "write-file"),
    MenuItem::new("File", "save", "Save All…", "save-some-buffers"),
    MenuItem::new("File", "close", "Close Buffer…", "kill-buffer").titled_by(buffers::close_buffer_title),
    MenuItem::new("Window", "windows", "Split Below", "split-window-below"),
    MenuItem::new("Window", "windows", "Split Right", "split-window-right"),
    MenuItem::new("Window", "windows", "Delete Window", "delete-window")
        .enabled_when(windows::has_other_windows),
    MenuItem::new("Window", "windows", "Delete Other Windows", "delete-other-windows")
        .enabled_when(windows::has_other_windows),
    MenuItem::new("Window", "windows", "Other Window", "other-window"),
    MenuItem::new("Window", "windows", "Switch to Buffer in Other Window…", "switch-to-buffer-other-window"),
    MenuItem::new("Window", "windows", "Open in Other Window…", "find-file-other-window"),
    MenuItem::new("Window", "windows", "Taller", "enlarge-window")
        .enabled_when(windows::has_other_windows),
    MenuItem::new("Window", "windows", "Wider", "enlarge-window-horizontally")
        .enabled_when(windows::has_other_windows),
    MenuItem::new("Window", "windows", "Narrower", "shrink-window-horizontally")
        .enabled_when(windows::has_other_windows),
    MenuItem::new("Window", "windows", "Balance Windows", "balance-windows")
        .enabled_when(windows::has_other_windows),
];

impl Plugin for FilesPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry.menu_items.extend_from_slice(MENU_ITEMS);
        registry.settings_hooks.push(Rc::new(read_settings));
        registry
            .post_command_hooks
            .push(Rc::new(revert::follow_files));
    }
}

/// Indexes scan again once `files.toml` changes what they leave out.
fn read_settings(editor: &mut Editor) {
    let settings = editor.settings_section::<FilesSettings>();
    editor.plugin_state::<KillBufferPrompt>().0 = settings.kill_buffer_prompt;
    editor.plugin_state::<AutoRevert>().enabled = settings.auto_revert;
    editor.plugin_state::<FileIndexes>().set_settings(settings);
    let windows = editor.settings_section::<WindowsSettings>();
    *editor.plugin_state::<WindowsSettings>() = windows;
}

#[cfg(test)]
mod tests;
