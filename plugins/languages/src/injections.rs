use remacs_syntax::Injections;

use crate::modes::{HTML, JAVASCRIPT, JSX, TSX, TYPESCRIPT};

const HTML_INJECTIONS: &str = include_str!("../queries/html/injections.scm");
const TEMPLATE_INJECTIONS: &str = include_str!("../queries/ecmascript/injections.scm");

pub const INJECTIONS: &[Injections] = &[
    Injections {
        host: HTML,
        query: HTML_INJECTIONS,
        documents: true,
    },
    Injections {
        host: JAVASCRIPT,
        query: TEMPLATE_INJECTIONS,
        documents: false,
    },
    Injections {
        host: JSX,
        query: TEMPLATE_INJECTIONS,
        documents: false,
    },
    Injections {
        host: TYPESCRIPT,
        query: TEMPLATE_INJECTIONS,
        documents: false,
    },
    Injections {
        host: TSX,
        query: TEMPLATE_INJECTIONS,
        documents: false,
    },
];
