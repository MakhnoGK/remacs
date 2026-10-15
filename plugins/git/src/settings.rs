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
}

impl Default for GitSettings {
    fn default() -> Self {
        Self {
            untracked_max_bytes: 1024 * 1024,
            untracked_max_files: 500,
        }
    }
}

impl Section for GitSettings {
    const NAME: &str = "git";

    fn check(&self) -> Result<(), String> {
        match self.untracked_max_files {
            0 => Err("untracked-max-files is 0".into()),
            _ => Ok(()),
        }
    }
}

pub fn read_settings(editor: &mut Editor) {
    let settings = editor.settings_section::<GitSettings>();
    *editor.plugin_state::<GitSettings>() = settings;
}
