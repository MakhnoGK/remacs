//! Grammars downloaded as WebAssembly (ADR 0087), compiled and run by wasmtime: sandboxed,
//! and no C compiler is needed to add one. Each is loaded once and kept for the run; a
//! parser gets a store of its own the first time it is given one.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use tree_sitter::wasmtime::Engine;
use tree_sitter::{Language, Parser, WasmStore};

static ENGINE: LazyLock<Engine> = LazyLock::new(Engine::default);

/// By the name of their file, with which they were loaded.
static LOADED: Mutex<BTreeMap<String, Language>> = Mutex::new(BTreeMap::new());

/// Loads the grammar called `name` from `path`, a `.wasm` file tree-sitter built.
pub fn load(name: &str, path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut store = WasmStore::new(&ENGINE).map_err(|error| error.to_string())?;
    // tree-sitter names a grammar's entry point after it: `tree_sitter_<name>`.
    let language = store
        .load_language(name, &bytes)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    LOADED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(name.to_owned(), language);
    Ok(())
}

pub(crate) fn loaded(name: &str) -> Option<Language> {
    LOADED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(name)
        .cloned()
}

/// `parser.set_language`, with a store to run `language` in if it is WebAssembly.
pub(crate) fn set_language(parser: &mut Parser, language: &Language) -> bool {
    if language.is_wasm() {
        let store = match parser.take_wasm_store() {
            Some(store) => store,
            None => match WasmStore::new(&ENGINE) {
                Ok(store) => store,
                Err(_) => return false,
            },
        };
        if parser.set_wasm_store(store).is_err() {
            return false;
        }
    }
    parser.set_language(language).is_ok()
}
