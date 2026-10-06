//! Where remacs keeps what it stores of its own, and the settings read from there.
//!
//! Settings are split into files, one section each: `config/ui.toml` is the `ui` section.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::de::DeserializeOwned;

/// `$XDG_CONFIG_HOME/remacs`, else `~/.config/remacs`.
pub fn home() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("remacs"))
}

/// The directory of the settings files.
pub fn config_dir(home: &Path) -> PathBuf {
    home.join("config")
}

/// The directory of what lasts between sessions, e.g. the minibuffer history.
pub fn state_dir(home: &Path) -> PathBuf {
    home.join("state")
}

/// Calls `on_change`, on a background thread, whenever a settings file in `dir` is
/// written, created or removed; until the watcher is dropped. Creates `dir`, so that files
/// added to it later are seen too.
pub fn watch(
    dir: &Path,
    mut on_change: impl FnMut() + Send + 'static,
) -> notify::Result<RecommendedWatcher> {
    fs::create_dir_all(dir)?;
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        if event.is_ok_and(|event| changes_settings(&event)) {
            on_change();
        }
    })?;
    // Editors often save by renaming a new file over the old one, so the directory is
    // watched, not the files.
    watcher.watch(dir, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

fn changes_settings(event: &Event) -> bool {
    !matches!(event.kind, EventKind::Access(_))
        && event
            .paths
            .iter()
            .any(|path| path.extension() == Some(OsStr::new("toml")))
}

/// Settings read from one file; what is left out keeps its default.
pub trait Section: DeserializeOwned + Default {
    /// The file's name without `.toml`.
    const NAME: &str;

    /// Rejects values that parse but make no sense, e.g. a font size of 0.
    fn check(&self) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsError {
    pub file: PathBuf,
    /// The 1-based line the error is on, when known.
    pub line: Option<usize>,
    pub message: String,
}

/// Names only the file: all settings files are in one directory, and the message has to
/// fit the status line.
impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        let name = self.file.file_name().unwrap_or(self.file.as_os_str());
        write!(formatter, "{}", name.display())?;
        if let Some(line) = self.line {
            write!(formatter, ":{line}")?;
        }
        write!(formatter, ": {}", self.message)
    }
}

struct SectionFile {
    path: PathBuf,
    text: String,
}

/// The settings files of a directory, parsed only when their section is asked for, so a
/// file nobody reads yet costs nothing and cannot fail.
#[derive(Default)]
pub struct Settings {
    files: BTreeMap<String, SectionFile>,
}

impl Settings {
    /// A missing directory means all defaults; unreadable files are left out and reported.
    pub fn load(dir: &Path) -> (Self, Vec<SettingsError>) {
        let mut files = Vec::new();
        let mut errors = Vec::new();
        let Ok(entries) = fs::read_dir(dir) else {
            return (Self::default(), errors);
        };
        for path in entries.flatten().map(|entry| entry.path()) {
            if path.extension() != Some(OsStr::new("toml")) {
                continue;
            }
            match fs::read_to_string(&path) {
                Ok(text) => files.push((path, text)),
                Err(error) => errors.push(SettingsError {
                    file: path,
                    line: None,
                    message: error.to_string(),
                }),
            }
        }
        (Self::from_files(files), errors)
    }

    /// Settings from files' paths and texts, each file a section named after it.
    pub fn from_files(files: impl IntoIterator<Item = (PathBuf, String)>) -> Self {
        let files = files
            .into_iter()
            .filter_map(|(path, text)| {
                let name = path.file_stem()?.to_str()?.to_owned();
                Some((name, SectionFile { path, text }))
            })
            .collect();
        Self { files }
    }

    /// The section's settings; when its file does not parse or check, the error and the
    /// defaults.
    pub fn section<T: Section>(&self) -> Result<T, (SettingsError, T)> {
        let Some(file) = self.files.get(T::NAME) else {
            return Ok(T::default());
        };
        let error = |line, message| SettingsError {
            file: file.path.clone(),
            line,
            message,
        };
        let section = toml::from_str::<T>(&file.text).map_err(|parse_error| {
            let line = parse_error
                .span()
                .map(|span| line_at(&file.text, span.start));
            (error(line, parse_error.message().to_owned()), T::default())
        })?;
        match section.check() {
            Ok(()) => Ok(section),
            Err(message) => Err((error(None, message), T::default())),
        }
    }
}

fn line_at(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].matches('\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(default, deny_unknown_fields)]
    struct Sample {
        size: f32,
        name: String,
    }

    impl Default for Sample {
        fn default() -> Self {
            Self {
                size: 13.0,
                name: "default".to_owned(),
            }
        }
    }

    impl Section for Sample {
        const NAME: &str = "sample";

        fn check(&self) -> Result<(), String> {
            if self.size > 0.0 {
                Ok(())
            } else {
                Err("size must be above 0".to_owned())
            }
        }
    }

    /// `test` keeps the directories of tests running at once apart.
    fn settings_with(
        test: &str,
        files: &[(&str, &str)],
    ) -> (PathBuf, Settings, Vec<SettingsError>) {
        let dir = std::env::temp_dir().join(format!("remacs-config-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for (name, text) in files {
            fs::write(dir.join(name), text).unwrap();
        }
        let (settings, errors) = Settings::load(&dir);
        (dir, settings, errors)
    }

    #[test]
    fn a_missing_directory_gives_defaults() {
        let (settings, errors) = Settings::load(Path::new("/nonexistent/remacs/config"));
        assert!(errors.is_empty());
        assert_eq!(settings.section::<Sample>(), Ok(Sample::default()));
    }

    #[test]
    fn a_section_keeps_defaults_for_what_it_leaves_out() {
        let (dir, settings, errors) = settings_with("partial", &[("sample.toml", "size = 15\n")]);
        assert!(errors.is_empty());
        let sample = settings.section::<Sample>().unwrap();
        assert_eq!(sample.size, 15.0);
        assert_eq!(sample.name, "default");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_malformed_section_reports_its_line_and_gives_defaults() {
        let (dir, settings, _) =
            settings_with("malformed", &[("sample.toml", "name = \"a\"\nsise = 15\n")]);
        let (error, sample) = settings.section::<Sample>().unwrap_err();
        assert_eq!(error.file, dir.join("sample.toml"));
        assert_eq!(error.line, Some(2));
        assert!(error.message.contains("sise"), "{}", error.message);
        assert_eq!(sample, Sample::default());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_section_that_fails_its_check_gives_defaults() {
        let (dir, settings, _) = settings_with("unchecked", &[("sample.toml", "size = 0\n")]);
        let (error, sample) = settings.section::<Sample>().unwrap_err();
        assert_eq!(error.message, "size must be above 0");
        assert_eq!(sample, Sample::default());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn other_files_are_not_sections() {
        let (dir, settings, _) = settings_with("other", &[("sample.toml.bak", "size = 0\n")]);
        assert_eq!(settings.section::<Sample>(), Ok(Sample::default()));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn writing_a_settings_file_is_seen_and_the_directory_is_created() {
        let dir = std::env::temp_dir()
            .join(format!("remacs-config-{}-watch", std::process::id()))
            .join("config");
        let _ = fs::remove_dir_all(&dir);
        let (changed, changes) = std::sync::mpsc::channel();
        let _watcher = watch(&dir, move || {
            let _ = changed.send(());
        })
        .unwrap();
        fs::write(dir.join("sample.toml"), "size = 15\n").unwrap();
        assert!(
            changes
                .recv_timeout(std::time::Duration::from_secs(5))
                .is_ok()
        );
        fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }
}
