//! Built-in plugin: a language server client, like Emacs eglot. It starts a server per
//! project, keeps it in sync with the buffers, passes its diagnostics on and answers the
//! completion, documentation and xref plugins.

mod client;
mod completion;
mod definition;
mod document;
mod hover;
mod markdown;
mod position;
mod snippet;
mod transport;
mod uri;

use std::rc::Rc;

use remacs_core::{Plugin, Registry};

/// How to run a language server and which major modes it serves.
pub struct LanguageServer {
    pub name: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    /// Major modes and their LSP language ids.
    pub languages: &'static [(&'static str, &'static str)],
    /// Files that mark a project root, like `Cargo.toml`.
    pub root_markers: &'static [&'static str],
    /// JSON for `initializationOptions`, like eglot's per-server settings.
    pub initialization_options: Option<&'static str>,
    /// The server reports diagnostics only when asked (`textDocument/diagnostic`). Servers
    /// that also publish them unasked are not asked, which would only duplicate them.
    pub pull_diagnostics: bool,
}

impl LanguageServer {
    fn language_id(&self, mode: &str) -> Option<&'static str> {
        self.languages
            .iter()
            .find(|(served, _)| *served == mode)
            .map(|&(_, language)| language)
    }
}

pub struct LspPlugin {
    servers: &'static [LanguageServer],
}

impl LspPlugin {
    pub const fn new(servers: &'static [LanguageServer]) -> Self {
        Self { servers }
    }
}

impl Plugin for LspPlugin {
    fn register(&self, registry: &mut Registry) {
        let servers = self.servers;
        registry
            .post_command_hooks
            .push(Rc::new(move |editor| document::sync(editor, servers)));
    }
}
