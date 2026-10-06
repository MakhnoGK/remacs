use std::collections::BTreeMap;
use std::path::PathBuf;

use remacs_core::{Editor, Section};
use serde::Deserialize;
use serde_json::Value;

use crate::LanguageServer;
use crate::document;

/// `lsp.toml`: changes to the built-in servers, by name.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct LspSettings {
    servers: BTreeMap<String, ServerSettings>,
}

impl Section for LspSettings {
    const NAME: &str = "lsp";
}

impl LspSettings {
    pub fn server(&self, name: &str) -> Option<&ServerSettings> {
        self.servers.get(name)
    }

    pub fn enabled(&self, name: &str) -> bool {
        self.server(name).is_none_or(|server| server.enabled)
    }
}

/// What is left out stays as built in.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct ServerSettings {
    enabled: bool,
    /// A path, `~/` allowed, or a name looked up on `PATH`.
    command: Option<String>,
    args: Option<Vec<String>>,
    initialization_options: Option<Value>,
    configuration: Option<Value>,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            command: None,
            args: None,
            initialization_options: None,
            configuration: None,
        }
    }
}

impl ServerSettings {
    pub fn command(&self) -> Option<PathBuf> {
        let command = self.command.as_deref()?;
        let home = std::env::var_os("HOME").map(PathBuf::from);
        Some(match (command.strip_prefix("~/"), home) {
            (Some(rest), Some(home)) => home.join(rest),
            _ => PathBuf::from(command),
        })
    }

    pub fn args(&self) -> Option<&[String]> {
        self.args.as_deref()
    }

    pub fn initialization_options(&self) -> Option<&Value> {
        self.initialization_options.as_ref()
    }

    pub fn configuration(&self) -> Option<&Value> {
        self.configuration.as_ref()
    }
}

/// Takes `lsp.toml` again; servers whose settings changed are restarted, or stopped when
/// turned off, on their next use.
pub(crate) fn read(editor: &mut Editor, servers: &'static [LanguageServer]) {
    let settings = editor.settings_section::<LspSettings>();
    if let Some(unknown) = settings
        .servers
        .keys()
        .find(|name| !servers.iter().any(|server| server.name == name.as_str()))
    {
        let known: Vec<_> = servers.iter().map(|server| server.name).collect();
        editor.set_message(format!(
            "lsp.toml: no server named `{unknown}`; there are {}",
            known.join(", ")
        ));
    }
    let previous = document::replace_settings(editor, settings.clone());
    for server in servers {
        if previous.server(server.name) != settings.server(server.name) {
            document::restart(editor, server.name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(text: &str) -> Result<LspSettings, String> {
        toml::from_str(text).map_err(|error| error.to_string())
    }

    #[test]
    fn servers_change_only_what_they_name() {
        let settings = parse(
            "[servers.rust-analyzer]\n\
             command = \"~/bin/rust-analyzer\"\n\
             initialization-options = { check = { command = \"clippy\" } }\n\
             [servers.eslint]\n\
             enabled = false\n",
        )
        .unwrap();
        let rust = settings.server("rust-analyzer").unwrap();
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        assert_eq!(rust.command(), Some(home.join("bin/rust-analyzer")));
        assert_eq!(rust.args(), None);
        assert_eq!(
            rust.initialization_options(),
            Some(&json!({ "check": { "command": "clippy" } }))
        );
        assert_eq!(rust.configuration(), None);
        assert!(settings.enabled("rust-analyzer"));
        assert!(!settings.enabled("eslint"));
        assert!(settings.enabled("clangd"));
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(parse("[servers.clangd]\ncomand = \"clangd\"\n").is_err());
        assert!(parse("[server.clangd]\n").is_err());
    }
}
