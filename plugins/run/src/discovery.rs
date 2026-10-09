//! What a project can run that it has no configuration for yet: the scripts of its
//! `package.json` files, its Cargo binaries and examples.

use std::fs;
use std::path::Path;

use crate::configuration::Configuration;
use crate::entry_points::{PACKAGE_JSON, RUST_MODE, entry_points};

/// Files read at most, so that a huge monorepo still opens the chooser at once.
const MAX_FILES: usize = 200;

/// Configurations for what the project's `files`, relative to `root`, can run.
pub fn discover(root: &Path, files: &[String]) -> Vec<Configuration> {
    files
        .iter()
        .filter(|file| may_start_programs(file))
        .take(MAX_FILES)
        .flat_map(|file| {
            let path = root.join(file);
            let mode = if file.ends_with(".rs") { RUST_MODE } else { "" };
            let text = fs::read_to_string(&path).unwrap_or_default();
            entry_points(&path, mode, &text, root)
        })
        .filter(|entry| !entry.temporary)
        .map(|entry| entry.configuration)
        .collect()
}

fn may_start_programs(file: &str) -> bool {
    let parts: Vec<&str> = file.split('/').collect();
    if parts.contains(&"node_modules") {
        return false;
    }
    match parts.as_slice() {
        [.., last] if *last == PACKAGE_JSON => true,
        [.., "src", "main.rs"] => true,
        [.., "src", "bin", name] | [.., "examples", name] => name.ends_with(".rs"),
        [.., "src", "bin", _, "main.rs"] | [.., "examples", _, "main.rs"] => true,
        _ => false,
    }
}
