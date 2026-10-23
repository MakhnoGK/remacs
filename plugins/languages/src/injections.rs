use remacs_syntax::Injections;

use crate::grammars::RUST_MACRO_ARGUMENTS;
use crate::modes::{HTML, JAVASCRIPT, JSX, MARKDOWN, RUST, SVELTE, TSX, TYPESCRIPT, VUE};

const HTML_INJECTIONS: &str = include_str!("../queries/html/injections.scm");
const TEMPLATE_INJECTIONS: &str = include_str!("../queries/ecmascript/injections.scm");
const RUST_INJECTIONS: &str = include_str!("../queries/rust/injections.scm");

/// Code in strings, comments and Markdown blocks: no document of its own.
const fn within(host: &'static str, query: &'static str) -> Injections {
    Injections {
        host,
        query,
        documents: false,
        servers: false,
    }
}

pub const INJECTIONS: &[Injections] = &[
    Injections {
        host: HTML,
        query: HTML_INJECTIONS,
        documents: true,
        servers: true,
    },
    within(JAVASCRIPT, TEMPLATE_INJECTIONS),
    within(JSX, TEMPLATE_INJECTIONS),
    within(TYPESCRIPT, TEMPLATE_INJECTIONS),
    within(TSX, TEMPLATE_INJECTIONS),
    within(MARKDOWN, include_str!("../queries/markdown/injections.scm")),
    within(RUST, RUST_INJECTIONS),
    within(RUST_MACRO_ARGUMENTS, RUST_INJECTIONS),
    // Their scripts use names only Vue's and Svelte's own servers know.
    Injections {
        host: VUE,
        query: include_str!("../queries/vue/injections.scm"),
        documents: true,
        servers: false,
    },
    Injections {
        host: SVELTE,
        query: include_str!("../queries/svelte/injections.scm"),
        documents: true,
        servers: false,
    },
];
