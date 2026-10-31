//! A section's settings file of the defaults, every setting commented: read in a buffer
//! of its own, or written into the settings directory to change from there.

use std::fs::File;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use remacs_config::{DEFAULT_FILES, config_dir, home};
use remacs_core::{Buffer, Candidate, CommandResult, Count, Editor, Prompt, filter_candidates};
use remacs_text::Text;

pub fn write_default_settings(editor: &mut Editor) -> CommandResult {
    let config = config_dir(&home().ok_or("No home directory")?);
    let prompt = Prompt {
        label: "Default settings".into(),
        placeholder: "Settings file to write…",
        hints: &[("↵", "Write"), ("C-g", "Cancel")],
        ..file_prompt()
    };
    let candidates: Vec<Candidate> = files()
        .map(|file| Candidate {
            tag: config.join(&file).exists().then_some("exists"),
            ..Candidate::new(file)
        })
        .collect();
    editor.read(
        prompt,
        move |_, input| filter_candidates(input, candidates.clone(), false),
        move |editor, file| {
            let path = write(&config, &file)?;
            editor.visit_file(&path).map_err(|error| format!("{file}: {error}"))?;
            editor.set_message(format!("Wrote {file} of the defaults"));
            Ok(())
        },
    )
}

/// Read-only, in `<file> (defaults)`; nothing is written.
pub fn view_default_settings(editor: &mut Editor) -> CommandResult {
    let prompt = Prompt {
        label: "Default settings".into(),
        placeholder: "Settings file to read…",
        hints: &[("↵", "Show"), ("C-g", "Cancel")],
        ..file_prompt()
    };
    let candidates: Vec<Candidate> = files().map(Candidate::new).collect();
    editor.read(
        prompt,
        move |_, input| filter_candidates(input, candidates.clone(), false),
        |editor, file| {
            let name = format!("{file} (defaults)");
            if !editor.switch_to_buffer(&name) {
                let mode = editor.mode_for(Some(Path::new(&file)));
                let text = Text::from(defaults(&file)?);
                editor.add_buffer(Buffer::new(name, text, mode).read_only());
            }
            Ok(())
        },
    )
}

fn file_prompt() -> Prompt {
    Prompt { require_match: true, count: Count::Total, ..Prompt::default() }
}

fn files() -> impl Iterator<Item = String> {
    DEFAULT_FILES.iter().map(|(name, _)| format!("{name}.toml"))
}

fn defaults(file: &str) -> Result<&'static str, String> {
    DEFAULT_FILES
        .iter()
        .find(|(name, _)| file.strip_suffix(".toml") == Some(*name))
        .map(|(_, text)| *text)
        .ok_or_else(|| format!("No settings file {file}"))
}

/// Never over a file already there, which holds the user's own settings.
fn write(config: &Path, file: &str) -> Result<PathBuf, String> {
    let text = defaults(file)?;
    let path = config.join(file);
    let mut created = std::fs::create_dir_all(config)
        .and_then(|()| File::create_new(&path))
        .map_err(|error| match error.kind() {
            ErrorKind::AlreadyExists => format!("{file} is there already, kept as it is"),
            _ => format!("{file}: {error}"),
        })?;
    if let Err(error) = created.write_all(text.as_bytes()) {
        // Half a file would read as broken settings and block writing it again.
        let _ = std::fs::remove_file(&path);
        return Err(format!("{file}: {error}"));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_file_of_the_defaults_once_and_keeps_one_already_there() {
        let config = std::env::temp_dir()
            .join(format!("remacs-frame-{}-defaults", std::process::id()))
            .join("config");
        let _ = std::fs::remove_dir_all(&config);
        let path = write(&config, "git.toml").unwrap();
        let (_, defaults) = DEFAULT_FILES.iter().find(|(name, _)| *name == "git").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), *defaults);
        std::fs::write(&path, "untracked-max-files = 5\n").unwrap();
        assert!(write(&config, "git.toml").unwrap_err().contains("kept"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "untracked-max-files = 5\n");
        assert!(write(&config, "nothing.toml").is_err());
        std::fs::remove_dir_all(config.parent().unwrap()).unwrap();
    }
}
