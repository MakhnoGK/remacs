//! Writing a section's settings file of the defaults, every setting commented, into the
//! settings directory to change from there.

use std::fs::File;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use remacs_config::{DEFAULT_FILES, config_dir, home};
use remacs_core::{Candidate, CommandResult, Count, Editor, Prompt, filter_candidates};

pub fn write_default_settings(editor: &mut Editor) -> CommandResult {
    let config = config_dir(&home().ok_or("No home directory")?);
    let prompt = Prompt {
        label: "Default settings".into(),
        placeholder: "Settings file to write…",
        require_match: true,
        count: Count::Total,
        hints: &[("↵", "Write"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    let candidates: Vec<Candidate> = DEFAULT_FILES
        .iter()
        .map(|(name, _)| {
            let file = format!("{name}.toml");
            Candidate {
                tag: config.join(&file).exists().then_some("exists"),
                ..Candidate::new(file)
            }
        })
        .collect();
    editor.read(
        prompt,
        move |_, input| filter_candidates(input, candidates.clone(), false),
        move |editor, file| {
            let path = write(&config, &file)?;
            editor
                .visit_file(&path)
                .map_err(|error| format!("{file}: {error}"))?;
            editor.set_message(format!("Wrote {file} of the defaults"));
            Ok(())
        },
    )
}

/// Never over a file already there, which holds the user's own settings.
fn write(config: &Path, file: &str) -> Result<PathBuf, String> {
    let (_, text) = DEFAULT_FILES
        .iter()
        .find(|(name, _)| file.strip_suffix(".toml") == Some(*name))
        .ok_or_else(|| format!("No settings file {file}"))?;
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
        let (_, defaults) = DEFAULT_FILES
            .iter()
            .find(|(name, _)| *name == "git")
            .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), *defaults);
        std::fs::write(&path, "untracked-max-files = 5\n").unwrap();
        assert!(write(&config, "git.toml").unwrap_err().contains("kept"));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "untracked-max-files = 5\n"
        );
        assert!(write(&config, "nothing.toml").is_err());
        std::fs::remove_dir_all(config.parent().unwrap()).unwrap();
    }
}
