//! What the project's file index leaves out: patterns of its own, in `.gitignore` syntax,
//! rather than the project's ignore files, so that files Git ignores but people search,
//! such as `.env`, are listed.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::sync::Arc;

use ignore::WalkBuilder;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use remacs_core::Section;
use serde::Deserialize;

/// `files.toml`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct FilesSettings {
    /// Files and folders left out of `find-file` and `project-search`, in `.gitignore`
    /// syntax; `.git/` always is.
    pub ignore: Vec<String>,
    /// Also leave out what the project's `.gitignore` and `.ignore` files do.
    pub use_gitignore: bool,
    /// `kill-buffer` asks which buffer to kill; off, it kills the current one.
    pub kill_buffer_prompt: bool,
}

/// Left out whatever the settings say.
const ALWAYS_IGNORED: &str = ".git/";

impl Default for FilesSettings {
    fn default() -> Self {
        let ignore = [
            "node_modules/",
            "target/",
            "dist/",
            "build/",
            ".next/",
            ".DS_Store",
        ];
        Self {
            ignore: ignore.map(str::to_owned).to_vec(),
            use_gitignore: false,
            kill_buffer_prompt: true,
        }
    }
}

impl Section for FilesSettings {
    const NAME: &str = "files";

    fn check(&self) -> Result<(), String> {
        let mut builder = GitignoreBuilder::new("/");
        for pattern in &self.ignore {
            builder
                .add_line(None, pattern)
                .map_err(|error| format!("ignore {pattern:?}: {error}"))?;
        }
        Ok(())
    }
}

impl FilesSettings {
    /// Tells the index caches of different settings of what is left out apart.
    pub fn key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        (&self.ignore, self.use_gitignore).hash(&mut hasher);
        hasher.finish()
    }
}

/// The settings' patterns for one project root.
pub struct Ignored {
    matcher: Gitignore,
    use_gitignore: bool,
}

impl Ignored {
    pub fn new(root: &Path, settings: &FilesSettings) -> Self {
        let mut builder = GitignoreBuilder::new(root);
        let _ = builder.add_line(None, ALWAYS_IGNORED);
        for pattern in &settings.ignore {
            // Checked when the settings were read.
            let _ = builder.add_line(None, pattern);
        }
        Self {
            matcher: builder.build().unwrap_or_else(|_| Gitignore::empty()),
            use_gitignore: settings.use_gitignore,
        }
    }

    pub fn uses_gitignore(&self) -> bool {
        self.use_gitignore
    }

    /// Whether `path`, under the root, or a folder it is in is left out.
    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        self.matcher
            .matched_path_or_any_parents(path, is_dir)
            .is_ignore()
    }

    /// A walk of `directory` that leaves out what is ignored; hidden files are listed.
    pub fn walker(self: &Arc<Self>, directory: &Path) -> WalkBuilder {
        let mut walker = WalkBuilder::new(directory);
        walker
            .hidden(false)
            .git_ignore(self.use_gitignore)
            .git_global(self.use_gitignore)
            .git_exclude(self.use_gitignore)
            .ignore(self.use_gitignore)
            .parents(self.use_gitignore);
        let ignored = Arc::clone(self);
        walker.filter_entry(move |entry| {
            let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
            !ignored.is_ignored(entry.path(), is_dir)
        });
        walker
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_file_reads_as_the_defaults() {
        remacs_core::assert_default_file::<FilesSettings>();
    }

    #[test]
    fn folders_and_globs_are_left_out_with_what_is_in_them() {
        let root = Path::new("/project");
        let settings = FilesSettings {
            ignore: vec!["node_modules/".into(), "*.min.js".into(), "/out".into()],
            ..FilesSettings::default()
        };
        let ignored = Ignored::new(root, &settings);
        let is_ignored = |path: &str, is_dir| ignored.is_ignored(&root.join(path), is_dir);
        assert!(is_ignored("node_modules", true));
        assert!(is_ignored("web/node_modules/react/index.js", false));
        assert!(!is_ignored("node_modules", false));
        assert!(is_ignored("web/app.min.js", false));
        assert!(is_ignored("out/a.txt", false));
        assert!(!is_ignored("web/out/a.txt", false));
        assert!(!is_ignored(".env", false));
    }

    #[test]
    fn bad_patterns_are_reported() {
        let settings = FilesSettings {
            ignore: vec!["{a".into()],
            ..FilesSettings::default()
        };
        assert!(settings.check().unwrap_err().contains("{a"));
    }
}
