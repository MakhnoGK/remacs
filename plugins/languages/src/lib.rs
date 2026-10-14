//! Built-in plugin: major modes, chosen by file name or extension, their tree-sitter
//! grammars and language servers.

mod grammars;
mod modes;
mod servers;

use remacs_core::{Plugin, Registry};

pub use grammars::GRAMMARS;
pub use modes::PRETTIER_MODES;
pub use servers::SERVERS;

pub struct LanguagesPlugin;

impl Plugin for LanguagesPlugin {
    fn register(&self, registry: &mut Registry) {
        for &mode in modes::MODES {
            registry.modes.add(mode);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::Path;

    use remacs_core::{Mode, Modes};
    use tree_sitter::{Language, Query};

    use super::*;
    use crate::modes::MODES;

    #[test]
    fn every_mode_has_a_grammar_whose_queries_compile() {
        for &Mode { name: mode, .. } in MODES {
            let grammar = GRAMMARS
                .iter()
                .find(|grammar| grammar.mode == mode)
                .unwrap();
            let language = Language::new(grammar.language);
            Query::new(&language, &grammar.highlights.concat())
                .unwrap_or_else(|error| panic!("{mode}: {error}"));
        }
    }

    #[test]
    fn each_extension_and_file_name_belongs_to_one_mode() {
        let mut seen = HashSet::new();
        for mode in MODES {
            for claimed in mode.extensions.iter().chain(mode.file_names) {
                assert!(seen.insert(claimed), "{claimed}");
            }
        }
    }

    #[test]
    fn modes_are_chosen_by_file_name_before_extension() {
        let mut modes = Modes::default();
        for &mode in MODES {
            modes.add(mode);
        }
        let mode_of = |path: &str| modes.name_for(Some(Path::new(path)));
        assert_eq!(mode_of("src/Makefile"), "Make");
        assert_eq!(mode_of("CMakeLists.txt"), "CMake");
        assert_eq!(mode_of("notes.txt"), "Fundamental");
        assert_eq!(mode_of("/Users/me/.zshrc"), "Bash");
        assert_eq!(mode_of("tsconfig.json"), "JSON");
    }

    #[test]
    fn servers_serve_known_modes() {
        let servers = remacs_lsp::catalog_servers(SERVERS).unwrap();
        for server in &servers {
            for (mode, _) in server.languages {
                let known = MODES.iter().any(|known| known.name == *mode);
                assert!(known, "{}: {mode}", server.name);
            }
        }
    }

    #[test]
    fn every_mode_with_a_language_server_has_one_built_in() {
        let servers = remacs_lsp::catalog_servers(SERVERS).unwrap();
        // Make and diff have no language server worth running.
        let without = ["Make", "Diff"];
        for mode in MODES.iter().filter(|mode| !without.contains(&mode.name)) {
            let served = servers
                .iter()
                .any(|server| server.languages.iter().any(|(name, _)| *name == mode.name));
            assert!(served, "{}", mode.name);
        }
    }
}
