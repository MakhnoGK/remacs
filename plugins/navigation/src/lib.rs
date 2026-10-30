//! Built-in plugin: going back and forward through where point was before each jump, to
//! the last changes, to a line by number, and to text on screen by label.

mod avy;
mod goto_line;
mod history;
mod last_change;

use std::rc::Rc;

use remacs_core::{Command, MenuItem, Plugin, Registry, Section};
use serde::Deserialize;

pub struct NavigationPlugin;

/// `navigation.toml`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct NavigationSettings {
    /// Places `navigate-back` keeps at most.
    pub history_size: usize,
    /// Lines point has to move across within a buffer for the move to be a jump.
    pub jump_lines: usize,
    /// The characters avy's labels are made of, the first on the nearest places.
    pub avy_keys: String,
    /// Milliseconds of quiet after the last character typed for `avy-goto-char-timer`
    /// before its matches are labelled.
    pub avy_timeout: u64,
}

impl Default for NavigationSettings {
    fn default() -> Self {
        Self { history_size: 100, jump_lines: 10, avy_keys: "asdfghjkl".into(), avy_timeout: 500 }
    }
}

impl Section for NavigationSettings {
    const NAME: &str = "navigation";

    fn check(&self) -> Result<(), String> {
        let mut keys: Vec<char> = self.avy_keys.chars().collect();
        keys.sort_unstable();
        keys.dedup();
        if self.history_size == 0 {
            Err("history-size is 0".into())
        } else if self.jump_lines == 0 {
            Err("jump-lines is 0".into())
        } else if keys.len() < 2 || keys.len() != self.avy_keys.chars().count() {
            Err("avy-keys needs two characters or more, each once".into())
        } else if keys.iter().any(|key| key.is_whitespace() || key.is_control()) {
            Err("avy-keys has a space or a control character".into())
        } else if self.avy_timeout > 5000 {
            Err("avy-timeout is over 5000".into())
        } else {
            Ok(())
        }
    }
}

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(history::NAVIGATE_BACK, "Go back to where point was before the last jump", history::navigate_back),
    Command::new(history::NAVIGATE_FORWARD, "Go forward again to where navigate-back left", history::navigate_forward),
    Command::new(last_change::GOTO_LAST_CHANGE, "Go to where the buffer was last changed, then to the changes before", last_change::goto_last_change),
    Command::new(last_change::GOTO_LAST_CHANGE_REVERSE, "Go back to the later change goto-last-change came from", last_change::goto_last_change_reverse),
    Command::new(goto_line::GOTO_LINE, "Go to a line by its number, or line:column", goto_line::goto_line),
    Command::new(avy::AVY_GOTO_CHAR_TIMER, "Jump to text on screen: type it, pause, then type the label on the place", avy::avy_goto_char_timer),
    Command::new(avy::AVY_GOTO_WORD, "Jump to a word on screen: type its first character, then its label", avy::avy_goto_word),
    Command::new(avy::AVY_GOTO_LINE, "Jump to a line on screen by the label at its start", avy::avy_goto_line),
];

const BINDINGS: &[(&str, &str)] = &[
    ("s-[", history::NAVIGATE_BACK),
    ("C-x C-SPC", history::NAVIGATE_BACK),
    ("s-]", history::NAVIGATE_FORWARD),
    ("C-x C-\\", last_change::GOTO_LAST_CHANGE),
    ("C-x C-|", last_change::GOTO_LAST_CHANGE_REVERSE),
    ("M-j", avy::AVY_GOTO_CHAR_TIMER),
    ("M-g w", avy::AVY_GOTO_WORD),
    ("M-g l", avy::AVY_GOTO_LINE),
    ("M-g g", goto_line::GOTO_LINE),
    ("M-g M-g", goto_line::GOTO_LINE),
    ("s-l", goto_line::GOTO_LINE),
];

#[rustfmt::skip]
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::new("Go", "history", "Back", history::NAVIGATE_BACK),
    MenuItem::new("Go", "history", "Forward", history::NAVIGATE_FORWARD),
    MenuItem::new("Go", "history", "Last Change", last_change::GOTO_LAST_CHANGE),
    MenuItem::new("Go", "jump", "Go to Line Number…", goto_line::GOTO_LINE),
    MenuItem::new("Go", "jump", "Jump to Text…", avy::AVY_GOTO_CHAR_TIMER),
    MenuItem::new("Go", "jump", "Jump to Word…", avy::AVY_GOTO_WORD),
    MenuItem::new("Go", "jump", "Jump to Line…", avy::AVY_GOTO_LINE),
];

impl Plugin for NavigationPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry.menu_items.extend_from_slice(MENU_ITEMS);
        registry.settings_hooks.push(Rc::new(|editor| {
            let settings = editor.settings_section::<NavigationSettings>();
            *editor.plugin_state() = settings;
        }));
        registry.post_command_hooks.push(Rc::new(history::update));
        registry.post_command_hooks.push(Rc::new(avy::update));
        registry.key_sinks.push(Rc::new(avy::take_key));
        registry.text_labels.push(Rc::new(avy::text_labels));
    }
}

#[cfg(test)]
mod tests;
