//! Built-in plugin: major modes for the initial languages, chosen by file extension.

use remacs_core::{Mode, Plugin, Registry};

pub struct LanguagesPlugin;

const MODES: &[(&str, &[&str])] = &[
    ("Rust", &["rs"]),
    ("C", &["c", "h"]),
    ("C++", &["cc", "cpp", "cxx", "hh", "hpp", "hxx"]),
    ("TypeScript", &["ts", "mts", "cts"]),
    ("TSX", &["tsx"]),
    ("JavaScript", &["js", "mjs", "cjs"]),
    ("JSX", &["jsx"]),
    ("HTML", &["html", "htm"]),
    ("CSS", &["css"]),
];

impl Plugin for LanguagesPlugin {
    fn register(&self, registry: &mut Registry) {
        for &(name, extensions) in MODES {
            registry.modes.add(Mode { name, extensions });
        }
    }
}
