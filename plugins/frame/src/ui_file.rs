//! Settings chosen by commands: written to `ui.toml`, which the application reads again
//! at once.

use std::fs;
use std::path::Path;

use remacs_config::Section;
use remacs_core::CommandResult;
use remacs_theme::DEFAULT;
use serde::Deserialize;

const UI_FILE: &str = "ui.toml";

/// The part of `ui.toml` this plugin reads; the rest is the application's.
#[derive(Default, Deserialize)]
pub struct Ui {
    #[serde(default)]
    pub theme: ThemeChoice,
    #[serde(default)]
    pub hints: HintsChoice,
}

#[derive(Deserialize)]
#[serde(default)]
pub struct ThemeChoice {
    pub name: String,
}

impl Default for ThemeChoice {
    fn default() -> Self {
        Self { name: DEFAULT.to_owned() }
    }
}

#[derive(Deserialize)]
#[serde(default)]
pub struct HintsChoice {
    pub show: bool,
}

impl Default for HintsChoice {
    fn default() -> Self {
        Self { show: true }
    }
}

impl Section for Ui {
    const NAME: &str = "ui";
}

/// Sets `key` of `[table]` in `ui.toml` to `value`, a TOML value as written.
pub fn set(config: &Path, table: &str, key: &str, value: &str) -> CommandResult {
    let file = config.join(UI_FILE);
    let text = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("{UI_FILE}: {error}").into()),
    };
    fs::create_dir_all(config)
        .and_then(|()| fs::write(&file, with_setting(&text, table, key, value)))
        .map_err(|error| format!("{UI_FILE}: {error}").into())
}

/// A TOML string of `text`.
pub fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `text` of `ui.toml` with `key = value` in `[table]`, the rest of it as it was.
fn with_setting(text: &str, table: &str, key: &str, value: &str) -> String {
    let header = format!("[{table}]");
    let setting = format!("{key} = {value}");
    let mut lines: Vec<&str> = text.lines().collect();
    let Some(start) = lines.iter().position(|line| line.trim() == header) else {
        let separator = match text {
            "" => "",
            _ if text.ends_with('\n') => "\n",
            _ => "\n\n",
        };
        return format!("{text}{separator}{header}\n{setting}\n");
    };
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map_or(lines.len(), |offset| start + 1 + offset);
    let existing = (start + 1..end).find(|&index| {
        let line = lines[index].trim_start();
        line.strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('='))
    });
    match existing {
        Some(index) => lines[index] = &setting,
        None => lines.insert(start + 1, &setting),
    }
    let mut changed = lines.join("\n");
    if text.ends_with('\n') || text.is_empty() {
        changed.push('\n');
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_theme(text: &str, name: &str) -> String {
        with_setting(text, "theme", "name", &quoted(name))
    }

    #[test]
    fn a_theme_table_without_a_name_reads_as_the_default() {
        let ui: Ui = toml::from_str("[theme]\nlight = \"paper\"\n").unwrap();
        assert_eq!(ui.theme.name, DEFAULT);
    }

    #[test]
    fn hints_show_unless_ui_toml_hides_them() {
        assert!(toml::from_str::<Ui>("[hints]\nside-room = 100\n").unwrap().hints.show);
        assert!(!toml::from_str::<Ui>("[hints]\nshow = false\n").unwrap().hints.show);
    }

    #[test]
    fn replaces_the_name_in_the_theme_table_and_keeps_the_rest() {
        let text = "[font]\nname = \"keep\"\n\n[theme]\n# A theme.\nname = \"modus-vivendi\"\n\n[glass]\nblur = 0\n";
        assert_eq!(
            with_theme(text, "modus-operandi"),
            "[font]\nname = \"keep\"\n\n[theme]\n# A theme.\nname = \"modus-operandi\"\n\n[glass]\nblur = 0\n"
        );
    }

    #[test]
    fn adds_the_name_to_a_theme_table_without_one() {
        assert_eq!(
            with_theme("[theme]\n[glass]\nblur = 0", "paper"),
            "[theme]\nname = \"paper\"\n[glass]\nblur = 0"
        );
    }

    #[test]
    fn adds_a_theme_table_to_a_file_without_one() {
        assert_eq!(with_theme("", "paper"), "[theme]\nname = \"paper\"\n");
        assert_eq!(
            with_theme("[glass]\nblur = 0\n", "paper"),
            "[glass]\nblur = 0\n\n[theme]\nname = \"paper\"\n"
        );
        assert_eq!(
            with_theme("[glass]\nblur = 0", "a\"b"),
            "[glass]\nblur = 0\n\n[theme]\nname = \"a\\\"b\"\n"
        );
    }

    #[test]
    fn a_key_is_not_taken_for_a_longer_one() {
        assert_eq!(
            with_setting("[hints]\nshow-all = 1\nshow = true\n", "hints", "show", "false"),
            "[hints]\nshow-all = 1\nshow = false\n"
        );
    }
}
