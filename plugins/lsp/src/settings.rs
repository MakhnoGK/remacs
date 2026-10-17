use std::collections::BTreeMap;

use remacs_core::{Editor, Section};
use remacs_tools::ToolKind;
use serde::Deserialize;

use crate::LanguageServer;
use crate::catalog::{self, ServerSpec};
use crate::document::{self, Clients};

/// `lsp.toml`: changes to the built-in servers, and servers of one's own, by name.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct LspSettings {
    /// A project wanting a server that is nowhere is offered its download, once.
    offer_downloads: bool,
    /// Code of other languages in a file, like CSS in HTML's `<style>`, goes to their
    /// servers too.
    embedded_code: bool,
    servers: BTreeMap<String, ServerSpec>,
}

impl Default for LspSettings {
    fn default() -> Self {
        Self {
            offer_downloads: true,
            embedded_code: true,
            servers: BTreeMap::new(),
        }
    }
}

impl Section for LspSettings {
    const NAME: &str = "lsp";
}

impl LspSettings {
    pub fn offer_downloads(&self) -> bool {
        self.offer_downloads
    }

    pub fn embedded_code(&self) -> bool {
        self.embedded_code
    }
}

/// The servers, built from `catalog` and `lsp.toml` the first time they are wanted.
pub(crate) fn servers(editor: &mut Editor, catalog: &str) -> &'static [LanguageServer] {
    match editor.plugin_state::<Clients>().servers() {
        Some(servers) => servers,
        None => read(editor, catalog),
    }
}

/// Takes `lsp.toml` again and builds the servers; those whose description changed are
/// restarted, or stopped when gone, on their next use.
pub(crate) fn read(editor: &mut Editor, catalog: &str) -> &'static [LanguageServer] {
    let settings = editor.settings_section::<LspSettings>();
    let specs = catalog::merge(catalog::built_in(catalog), &settings.servers);
    let mut errors = Vec::new();
    let servers: Vec<LanguageServer> = specs
        .iter()
        .filter_map(|(name, spec, own_command)| {
            spec.build(name, *own_command)
                .map_err(|error| errors.push(error))
                .ok()
        })
        .collect();
    let servers: &'static [LanguageServer] = servers.leak();
    let specs = specs
        .into_iter()
        .map(|(name, spec, _)| (name, spec))
        .collect();
    let previous = document::replace_catalog(editor, settings, servers, specs);
    for name in previous {
        document::restart(editor, name);
    }
    let tools: Vec<_> = servers.iter().filter_map(|server| server.tool).collect();
    remacs_tools::provide(editor, ToolKind::LanguageServer, &tools);
    if let Some(error) = errors.first() {
        editor.set_error(format!("lsp.toml: {error}"));
    }
    servers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<LspSettings, String> {
        toml::from_str(text).map_err(|error| error.to_string())
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(parse("[servers.clangd]\ncomand = \"clangd\"\n").is_err());
        assert!(parse("[server.clangd]\n").is_err());
        assert!(parse("offer-downloads = false\n").is_ok());
    }
}
