//! Built-in plugin: a language server client, like Emacs eglot. It starts a server per
//! project, keeps it in sync with the buffers, passes its diagnostics on, marks the colors
//! files name and answers the completion, documentation, xref and actions plugins.

mod catalog;
mod client;
mod code_action;
mod color;
mod completion;
mod definition;
mod document;
mod document_symbols;
/// Public for the benchmarks only.
#[doc(hidden)]
pub mod embedded;
mod file_rename;
mod hover;
mod markdown;
mod offer;
/// Public for the benchmarks only.
#[doc(hidden)]
pub mod position;
mod restart;
mod settings;
mod snippet;
mod symbol_rename;
mod symbols;
mod transport;
mod uri;
mod workspace_edit;

use std::path::Path;
use std::rc::Rc;

use remacs_config::state_dir;
use remacs_core::{Command, Editor, MenuItem, Plugin, Registry};
use remacs_tools::Tool;
use serde_json::Value;

use crate::catalog::When;
use crate::offer::Offers;

/// How to run a language server and which major modes it serves; built from the servers'
/// catalog.
#[derive(Debug, Default)]
pub struct LanguageServer {
    pub name: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    /// Major modes and their LSP language ids.
    pub languages: &'static [(&'static str, &'static str)],
    /// Files that mark a project root, like `Cargo.toml`.
    pub root_markers: &'static [&'static str],
    /// `initializationOptions`, like eglot's per-server settings.
    pub initialization_options: Option<Value>,
    /// Answered for every item the server asks for with `workspace/configuration`, like
    /// eglot's `eglot-workspace-configuration`; without it, the server's defaults.
    pub configuration: Option<Value>,
    /// The server reports diagnostics only when asked (`textDocument/diagnostic`). Servers
    /// that also publish them unasked are not asked, which would only duplicate them.
    pub pull_diagnostics: bool,
    /// The server says when it is loading the project (`experimental/serverStatus`), and
    /// is taken as loading from its start until it first does.
    pub reports_loading: bool,
    /// Runs the project's own copy from `node_modules/.bin` when it has one, as npx does,
    /// so its version matches the project's.
    pub node_modules: bool,
    /// The server runs only for projects where this holds, e.g. a linter's config is
    /// there; empty, for every file.
    pub when: When,
    /// Added to the server's environment for projects where `env_when` holds, e.g.
    /// `VP_VERSION`, without which oxlint and oxfmt ignore Vite+'s `vite.config.ts`.
    pub env: &'static [(&'static str, &'static str)],
    pub env_when: When,
    /// Formats its languages' files ahead of the language's own server, like oxfmt
    /// rather than tsc.
    pub formats_first: bool,
    /// Servers of one group are alternatives for the same languages, e.g. Python's:
    /// the first found serves, and a project lacking all of them is offered a choice.
    pub group: Option<&'static str>,
    /// What remacs downloads, when offered to, for a project wanting the server while
    /// `command` is on neither `PATH` nor in `node_modules`.
    pub tool: Option<&'static Tool>,
    /// A program set in `lsp.toml`, run as it is rather than looked for.
    pub own_program: Option<&'static Path>,
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
    /// The built-in servers' catalog, TOML.
    catalog: &'static str,
}

impl LspPlugin {
    pub const fn new(catalog: &'static str) -> Self {
        Self { catalog }
    }
}

impl Plugin for LspPlugin {
    fn register(&self, registry: &mut Registry) {
        let catalog = self.catalog;
        registry.post_command_hooks.push(Rc::new(move |editor| {
            let servers = settings::servers(editor, catalog);
            document::sync(editor, servers);
            file_rename::sync(editor, servers);
            symbol_rename::sync(editor, servers);
            symbols::sync(editor, servers);
            document_symbols::sync(editor, servers);
            offer::ask(editor);
        }));
        registry.decorators.push(Rc::new(document::swatch_spans));
        registry.settings_hooks.push(Rc::new(move |editor| {
            settings::read(editor, catalog);
        }));
        registry.commands.add(Command::new(
            "lsp-restart",
            "Stop a language server and start it again where it is wanted",
            restart::restart_server,
        ));
        registry.commands.add(Command::new(
            offer::DOWNLOAD,
            "Offer again to download the language servers this file wants and lacks",
            offer::offer_again,
        ));
        registry.menu_items.push(
            MenuItem::new("Edit", "code", "Download Language Servers", offer::DOWNLOAD)
                .enabled_when(offer::can_download),
        );
    }
}

/// The servers a catalog describes as they are built in, or why one cannot be built.
pub fn catalog_servers(catalog: &str) -> Result<Vec<LanguageServer>, String> {
    catalog::merge(catalog::built_in(catalog), &Default::default())
        .iter()
        .map(|(name, spec, _)| spec.build(name, false))
        .collect()
}

/// Keeps under `home`'s state directory which projects were offered which servers.
pub fn keep_offers_in(editor: &mut Editor, home: &Path) {
    let store = state_dir(home).join("language-servers.json");
    editor.plugin_state::<Offers>().keep_in(store);
}
