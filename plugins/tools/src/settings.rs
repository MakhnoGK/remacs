use std::collections::BTreeMap;

use remacs_core::Section;
use serde::Deserialize;

use crate::installers::Installer;

/// `tools.toml`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct ToolsSettings {
    /// Newer releases of the tools installed are looked for when the Tools panel opens.
    pub check_updates: bool,
    pub tools: BTreeMap<String, ToolSettings>,
    /// Installers of one's own, or changes to the built-in ones, by name.
    pub installers: BTreeMap<String, Installer>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct ToolSettings {
    /// The release to install, rather than the latest.
    pub version: Option<String>,
    /// A program of one's own, used instead of downloading one.
    pub path: Option<String>,
}

impl Default for ToolsSettings {
    fn default() -> Self {
        Self {
            check_updates: true,
            tools: BTreeMap::new(),
            installers: BTreeMap::new(),
        }
    }
}

impl ToolsSettings {
    pub fn of(&self, name: &str) -> Option<&ToolSettings> {
        self.tools.get(name)
    }
}

impl Section for ToolsSettings {
    const NAME: &str = "tools";
}
