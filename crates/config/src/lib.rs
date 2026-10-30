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
    let xdg = std::env::var_os("XDG_CONFIG_HOME").filter(|dir| !dir.is_empty()).map(PathBuf::from);
    let home_config = || Some(PathBuf::from(std::env::var_os("HOME")?).join(".config"));
    Some(xdg.or_else(home_config)?.join("remacs"))
}

macro_rules! home_dirs {
    ($($(#[$doc:meta])* $function:ident: $name:literal,)*) => {$(
        $(#[$doc])*
        pub fn $function(home: &Path) -> PathBuf {
            home.join($name)
        }
    )*};
}

home_dirs! {
    /// The directory of the settings files.
    config_dir: "config",
    /// The directory of the user's themes, one file each.
    themes_dir: "themes",
    /// The directory of what lasts between sessions, e.g. the minibuffer history.
    state_dir: "state",
    /// The directory of what can be rebuilt, e.g. a project's file index.
    cache_dir: "cache",
    /// The directory of run configurations, one file per project.
    run_dir: "run",
}

/// A project's own directory under `cache`.
pub fn project_cache_dir(cache: &Path, root: &Path) -> PathBuf {
    cache.join("projects").join(project_name(root))
}

/// What a project's own files and directories are called: after its root for people, and
/// a hash of the root's path for uniqueness.
pub fn project_name(root: &Path) -> String {
    let name = root.file_name().map_or_else(|| "root".into(), |name| name.to_string_lossy());
    let hash = fnv1a(root.as_os_str().as_encoded_bytes());
    format!("{name}-{hash:016x}")
}

/// Stable across runs and Rust versions, unlike `DefaultHasher`.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
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
    !matches!(event.kind, EventKind::Access(_)) && event.paths.iter().any(|path| is_toml(path))
}

/// Each section's settings file of the defaults, with comments on every setting:
/// `config/defaults/` in the sources.
#[rustfmt::skip]
pub const DEFAULT_FILES: &[(&str, &str)] = &[
    ("debug", include_str!("../../../config/defaults/debug.toml")),
    ("editing", include_str!("../../../config/defaults/editing.toml")),
    ("environment", include_str!("../../../config/defaults/environment.toml")),
    ("files", include_str!("../../../config/defaults/files.toml")),
    ("format", include_str!("../../../config/defaults/format.toml")),
    ("git", include_str!("../../../config/defaults/git.toml")),
    ("keys", include_str!("../../../config/defaults/keys.toml")),
    ("logs", include_str!("../../../config/defaults/logs.toml")),
    ("lsp", include_str!("../../../config/defaults/lsp.toml")),
    ("modes", include_str!("../../../config/defaults/modes.toml")),
    ("navigation", include_str!("../../../config/defaults/navigation.toml")),
    ("run", include_str!("../../../config/defaults/run.toml")),
    ("search", include_str!("../../../config/defaults/search.toml")),
    ("syntax", include_str!("../../../config/defaults/syntax.toml")),
    ("terminal", include_str!("../../../config/defaults/terminal.toml")),
    ("tools", include_str!("../../../config/defaults/tools.toml")),
    ("ui", include_str!("../../../config/defaults/ui.toml")),
    ("windows", include_str!("../../../config/defaults/windows.toml")),
];

/// For each section's tests: panics unless its file in [`DEFAULT_FILES`] reads, as
/// settings files are read, to exactly `T::default()`.
pub fn assert_default_file<T: Section + PartialEq + fmt::Debug>() {
    let file = format!("{}.toml", T::NAME);
    let default_file = DEFAULT_FILES.iter().find(|(name, _)| *name == T::NAME);
    let (_, text) = default_file.unwrap_or_else(|| panic!("no {file} in DEFAULT_FILES"));
    let settings = Settings::from_files([(PathBuf::from(&file), (*text).to_owned())]);
    let section = settings.section::<T>().map_err(|(error, _)| error.to_string());
    assert_eq!(section, Ok(T::default()), "{file}");
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
        let (mut files, mut errors) = (Vec::new(), Vec::new());
        let paths = fs::read_dir(dir).into_iter().flatten().flatten().map(|entry| entry.path());
        for path in paths.filter(|path| is_toml(path)) {
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
        let section_file = |(path, text): (PathBuf, String)| {
            Some((path.file_stem()?.to_str()?.to_owned(), SectionFile { path, text }))
        };
        Self { files: files.into_iter().filter_map(section_file).collect() }
    }

    /// The section's settings; when its file does not parse or check, the error and the
    /// defaults.
    pub fn section<T: Section>(&self) -> Result<T, (SettingsError, T)> {
        let Some(file) = self.files.get(T::NAME) else {
            return Ok(T::default());
        };
        let error = |line, message| SettingsError { file: file.path.clone(), line, message };
        let section = toml::from_str::<T>(&file.text).map_err(|parse_error| {
            let line = parse_error.span().map(|span| line_at(&file.text, span.start));
            (error(line, parse_error.message().to_owned()), T::default())
        })?;
        section.check().map_err(|message| (error(None, message), T::default()))?;
        Ok(section)
    }
}

fn is_toml(path: &Path) -> bool {
    path.extension() == Some(OsStr::new("toml"))
}

fn line_at(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].matches('\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[test]
    fn each_project_has_its_own_stable_cache_directory() {
        let cache = Path::new("/cache");
        let remacs = project_cache_dir(cache, Path::new("/src/remacs"));
        assert_eq!(remacs, project_cache_dir(cache, Path::new("/src/remacs")));
        assert!(remacs.starts_with("/cache/projects"));
        let name = remacs.file_name().unwrap().to_str().unwrap();
        let hash = name.strip_prefix("remacs-");
        assert!(hash.is_some_and(|hash| hash.len() == 16), "{name}");
        assert_ne!(remacs, project_cache_dir(cache, Path::new("/old/remacs")));
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(default, deny_unknown_fields)]
    struct Sample {
        size: f32,
        name: String,
    }

    impl Default for Sample {
        fn default() -> Self {
            Self { size: 13.0, name: "default".to_owned() }
        }
    }

    impl Section for Sample {
        const NAME: &str = "sample";

        fn check(&self) -> Result<(), String> {
            let above_zero = self.size > 0.0;
            above_zero.then_some(()).ok_or("size must be above 0".to_owned())
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
    fn every_file_of_the_defaults_is_listed_once() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/defaults");
        let on_disk = fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().file_name());
        let mut on_disk: Vec<String> = on_disk.map(|name| name.into_string().unwrap()).collect();
        on_disk.sort();
        let listed = DEFAULT_FILES.iter().map(|(name, _)| format!("{name}.toml"));
        assert_eq!(listed.collect::<Vec<_>>(), on_disk);
    }

    /// The `NAME` of each `impl Section` in the sources under `dir`.
    fn section_names(dir: &Path, names: &mut Vec<String>) {
        let paths = fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().path());
        for path in paths {
            if path.is_dir() {
                section_names(&path, names);
            } else if path.extension() == Some(OsStr::new("rs")) {
                let text = fs::read_to_string(&path).unwrap();
                let lines: Vec<&str> = text.lines().collect();
                for pair in lines.windows(2) {
                    let name = pair[1].trim().strip_prefix("const NAME: &str = \"");
                    if let (true, Some(name)) = (pair[0].contains("Section for "), name) {
                        names.push(name.trim_end_matches("\";").to_owned());
                    }
                }
            }
        }
    }

    #[test]
    fn every_section_has_a_file_of_the_defaults() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut names = Vec::new();
        for crates in ["crates", "plugins"] {
            for krate in fs::read_dir(root.join(crates)).unwrap() {
                let krate = krate.unwrap().path();
                if !krate.ends_with("config") && krate.join("src").is_dir() {
                    section_names(&krate.join("src"), &mut names);
                }
            }
        }
        assert!(names.len() >= DEFAULT_FILES.len(), "{names:?}");
        let listed = |name: &str| DEFAULT_FILES.iter().any(|(file, _)| *file == name);
        for name in names {
            assert!(listed(&name), "no config/defaults/{name}.toml");
        }
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
        let seen = changes.recv_timeout(std::time::Duration::from_secs(5));
        assert!(seen.is_ok());
        fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }
}
