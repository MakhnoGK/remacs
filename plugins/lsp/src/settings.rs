use std::collections::BTreeMap;

use remacs_core::{Editor, Section};
use remacs_syntax::SyntaxSettings;
use remacs_tools::ToolKind;
use serde::Deserialize;

use crate::LanguageServer;
use crate::catalog::{self, ServerSpec};
use crate::document::{self, Clients};
use crate::semantic::{self, Painting};

/// `lsp.toml`: changes to the built-in servers, and servers of one's own, by name.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct LspSettings {
    /// A project wanting a server that is nowhere is offered its download, once.
    offer_downloads: bool,
    /// Code of other languages in a file, like CSS in HTML's `<style>`, goes to their
    /// servers too.
    embedded_code: bool,
    /// Typing a call's opening bracket, or what else its server says starts its
    /// arguments, shows the call's signature; off, only `signature-help` does.
    signature_help_on_typing: bool,
    /// What servers know each name to be paints over tree-sitter's faces (ADR 0088).
    semantic_tokens: bool,
    /// Changes to which capture each token type takes, over `semantic::CAPTURES`.
    semantic_token_captures: BTreeMap<String, String>,
    servers: BTreeMap<String, ServerSpec>,
}

impl Default for LspSettings {
    fn default() -> Self {
        Self {
            offer_downloads: true,
            embedded_code: true,
            signature_help_on_typing: true,
            semantic_tokens: true,
            semantic_token_captures: BTreeMap::new(),
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

    pub fn signature_help_on_typing(&self) -> bool {
        self.signature_help_on_typing
    }

    /// How semantic tokens are painted, with `syntax`'s level and variables.
    pub fn painting(&self, syntax: &SyntaxSettings) -> Painting {
        Painting {
            enabled: self.semantic_tokens,
            captures: semantic::captures(&self.semantic_token_captures),
            level: syntax.highlight_level,
            variables: syntax.local_variables,
        }
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
    let painting = settings.painting(&editor.settings_section::<SyntaxSettings>());
    editor.plugin_state::<Clients>().paint_with(painting);
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
    fn the_default_file_reads_as_the_defaults() {
        remacs_config::assert_default_file::<LspSettings>();
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(parse("[servers.clangd]\ncomand = \"clangd\"\n").is_err());
        assert!(parse("[server.clangd]\n").is_err());
        assert!(parse("offer-downloads = false\n").is_ok());
    }
}
