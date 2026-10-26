use remacs_core::{Editor, Section};
use serde::Deserialize;

/// `git.toml`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct GitSettings {
    /// Untracked files larger than this are summarized, not shown as added lines.
    pub untracked_max_bytes: u64,
    /// Files an unfolded untracked directory lists at most.
    pub untracked_max_files: usize,
    /// A commit's diffs kept at most; a file whose diff does not fit is summarized.
    pub commit_max_bytes: u64,
    /// The status's staged diffs kept at most, and its unstaged ones; as for a commit.
    pub changes_max_bytes: u64,
    /// Characters of an author's name a blame buffer shows.
    pub blame_author_width: usize,
}

impl Default for GitSettings {
    fn default() -> Self {
        Self {
            untracked_max_bytes: 1024 * 1024,
            untracked_max_files: 500,
            commit_max_bytes: 8 * 1024 * 1024,
            changes_max_bytes: 8 * 1024 * 1024,
            blame_author_width: 12,
        }
    }
}

impl Section for GitSettings {
    const NAME: &str = "git";

    fn check(&self) -> Result<(), String> {
        if self.untracked_max_files == 0 {
            return Err("untracked-max-files is 0".into());
        }
        if self.blame_author_width == 0 {
            return Err("blame-author-width is 0".into());
        }
        Ok(())
    }
}

pub fn read_settings(editor: &mut Editor) {
    let settings = editor.settings_section::<GitSettings>();
    *editor.plugin_state::<GitSettings>() = settings;
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_default_file_reads_as_the_defaults() {
        remacs_core::assert_default_file::<super::GitSettings>();
    }
}
