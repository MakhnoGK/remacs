use std::collections::BTreeMap;

use remacs_core::Section;
use serde::Deserialize;

use crate::catalog;

/// `tools.toml`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct ToolsSettings {
    /// Newer releases of the tools installed are looked for when the Tools panel opens.
    pub check_updates: bool,
    pub tools: BTreeMap<String, ToolSettings>,
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

    fn check(&self) -> Result<(), String> {
        match self.tools.keys().find(|name| catalog::find(name).is_none()) {
            Some(name) => Err(format!("no tool is called {name}")),
            None => Ok(()),
        }
    }
}
