//! Choosing the theme: written to `ui.toml`, which the application reads again at once.

use std::fs;
use std::path::Path;

use remacs_config::{Section, config_dir, home, themes_dir};
use remacs_core::{Candidate, CommandResult, Count, Editor, Prompt, filter_candidates};
use remacs_theme::{DEFAULT, Theme, names};
use serde::Deserialize;

const UI_FILE: &str = "ui.toml";
const THEME_TABLE: &str = "[theme]";

/// The part of `ui.toml` this plugin reads; the rest is the application's.
#[derive(Default, Deserialize)]
struct Ui {
    #[serde(default)]
    theme: ThemeChoice,
}

#[derive(Deserialize)]
struct ThemeChoice {
    name: String,
}

impl Default for ThemeChoice {
    fn default() -> Self {
        Self {
            name: DEFAULT.to_owned(),
        }
    }
}

impl Section for Ui {
    const NAME: &str = "ui";
}

pub fn select_theme(editor: &mut Editor) -> CommandResult {
    let home = home().ok_or("No home directory")?;
    let current = editor.settings_section::<Ui>().theme.name;
    let prompt = Prompt {
        label: "Theme".into(),
        placeholder: "Choose a theme…",
        require_match: true,
        count: Count::Total,
        default: Some(current.clone()),
        hints: &[("↵", "Use"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    let themes = themes_dir(&home);
    let candidates: Vec<Candidate> = names(Some(&themes))
        .into_iter()
        .map(|name| {
            let annotation = Theme::load(&name, Some(&themes))
                .map_or_else(|error| error.to_string(), |theme| theme.name);
            Candidate {
                annotation,
                tag: (name == current).then_some("current"),
                ..Candidate::new(name)
            }
        })
        .collect();
    editor.read(
        prompt,
        move |_, input| filter_candidates(input, candidates.clone(), false),
        move |editor, name| {
            choose(&config_dir(&home), &name)?;
            editor.set_message(format!("Theme {name}"));
            Ok(())
        },
    )
}

fn choose(config: &Path, name: &str) -> CommandResult {
    let file = config.join(UI_FILE);
    let text = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("{UI_FILE}: {error}").into()),
    };
    fs::create_dir_all(config)
        .and_then(|()| fs::write(&file, with_theme(&text, name)))
        .map_err(|error| format!("{UI_FILE}: {error}").into())
}

/// `text` of `ui.toml` naming `name` as the theme, the rest of it as it was.
fn with_theme(text: &str, name: &str) -> String {
    let setting = format!(
        "name = \"{}\"",
        name.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let mut lines: Vec<&str> = text.lines().collect();
    let Some(table) = lines.iter().position(|line| line.trim() == THEME_TABLE) else {
        let separator = match text {
            "" => "",
            _ if text.ends_with('\n') => "\n",
            _ => "\n\n",
        };
        return format!("{text}{separator}{THEME_TABLE}\n{setting}\n");
    };
    let end = lines[table + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map_or(lines.len(), |offset| table + 1 + offset);
    let existing = (table + 1..end).find(|&index| {
        let line = lines[index].trim_start();
        line.strip_prefix("name")
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    });
    match existing {
        Some(index) => lines[index] = &setting,
        None => lines.insert(table + 1, &setting),
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
}
