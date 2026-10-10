use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use remacs_core::{CursorShape, Key, Section, parse_sequence};
use serde::Deserialize;

const PANEL_HEIGHTS: RangeInclusive<f32> = 120.0..=1200.0;
const SCROLLBACK_LINES: RangeInclusive<usize> = 0..=1_000_000;
/// Milliseconds.
const KILL_TIMEOUTS: RangeInclusive<u64> = 0..=60_000;

/// `terminal.toml`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct TerminalSettings {
    /// The shell new terminals run: a path or a name on `PATH`; empty is `$SHELL`, else
    /// the user's login shell.
    pub shell: String,
    pub args: Vec<String>,
    /// Where `terminal` and `terminal-new` show a terminal.
    pub open_in: Placement,
    /// Pixels the panel takes under the text.
    pub panel_height: f32,
    /// Lines a terminal keeps above its screen.
    pub scrollback_lines: usize,
    /// Where a new terminal starts.
    pub directory: StartDirectory,
    pub cursor: CursorStyle,
    /// Close a terminal as soon as its shell exits, whatever its code.
    pub close_on_exit: bool,
    /// Milliseconds a closed terminal's programs have to end after the hangup signal
    /// before they are killed.
    pub kill_timeout: u64,
    /// Keys remacs keeps while a terminal has the keys, in Emacs notation; a prefix
    /// keeps every sequence it starts. ⌘ keys always stay with remacs.
    pub editor_keys: Vec<String>,
    /// Set for every shell started; a table here replaces this whole one.
    pub env: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Placement {
    #[default]
    Panel,
    Buffer,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StartDirectory {
    /// The project of the window buffer.
    #[default]
    Project,
    /// The window buffer's own directory.
    File,
    Home,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CursorStyle {
    #[default]
    Block,
    Bar,
    Underline,
}

impl CursorStyle {
    pub fn shape(self) -> CursorShape {
        match self {
            CursorStyle::Block => CursorShape::Block,
            CursorStyle::Bar => CursorShape::Bar,
            CursorStyle::Underline => CursorShape::Underline,
        }
    }
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            shell: String::new(),
            args: vec!["--login".to_owned()],
            open_in: Placement::Panel,
            panel_height: 320.0,
            scrollback_lines: 10_000,
            directory: StartDirectory::Project,
            cursor: CursorStyle::Block,
            close_on_exit: false,
            kill_timeout: 2_000,
            editor_keys: ["C-x", "M-x", "C-`", "M-<f12>"].map(str::to_owned).to_vec(),
            env: [
                ("TERM", "xterm-256color"),
                ("COLORTERM", "truecolor"),
                ("TERM_PROGRAM", "remacs"),
            ]
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect(),
        }
    }
}

impl TerminalSettings {
    /// The first key of each of `editor-keys`.
    pub fn kept_keys(&self) -> Vec<Key> {
        self.editor_keys
            .iter()
            .filter_map(|keys| parse_sequence(keys)?.into_iter().next())
            .collect()
    }
}

impl Section for TerminalSettings {
    const NAME: &str = "terminal";

    fn check(&self) -> Result<(), String> {
        if !PANEL_HEIGHTS.contains(&self.panel_height) {
            return Err(format!(
                "panel-height must be {} to {}",
                PANEL_HEIGHTS.start(),
                PANEL_HEIGHTS.end()
            ));
        }
        if !SCROLLBACK_LINES.contains(&self.scrollback_lines) {
            return Err(format!(
                "scrollback-lines must be {} to {}",
                SCROLLBACK_LINES.start(),
                SCROLLBACK_LINES.end()
            ));
        }
        if !KILL_TIMEOUTS.contains(&self.kill_timeout) {
            return Err(format!(
                "kill-timeout must be {} to {}",
                KILL_TIMEOUTS.start(),
                KILL_TIMEOUTS.end()
            ));
        }
        if let Some(keys) = self
            .editor_keys
            .iter()
            .find(|keys| parse_sequence(keys).is_none_or(|keys| keys.is_empty()))
        {
            return Err(format!("editor-keys: {keys} is not a key"));
        }
        Ok(())
    }
}
