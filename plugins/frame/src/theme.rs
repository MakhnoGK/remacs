//! Choosing the theme: written to `ui.toml`.

use remacs_config::{config_dir, home, themes_dir};
use remacs_core::{Candidate, CommandResult, Count, Editor, Prompt, filter_candidates};
use remacs_theme::{SYSTEM, Theme, names};

use crate::ui_file::{self, Ui, quoted};

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
    let candidates: Vec<Candidate> = std::iter::once(SYSTEM.to_owned())
        .chain(names(Some(&themes)).into_iter().filter(|name| name != SYSTEM))
        .map(|name| {
            let annotation = match name.as_str() {
                SYSTEM => "Light or dark as macOS is, the themes in ui.toml".to_owned(),
                _ => Theme::load(&name, Some(&themes))
                    .map_or_else(|error| error.to_string(), |theme| theme.name),
            };
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
            ui_file::set(&config_dir(&home), "theme", "name", &quoted(&name))?;
            editor.set_message(format!("Theme {name}"));
            Ok(())
        },
    )
}
