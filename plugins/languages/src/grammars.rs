use remacs_syntax::Source::Built;
use remacs_syntax::{Grammar, Indents, LanguageFn, Locals, Source};

use crate::modes::*;

/// Our own queries, written from each grammar's node types (clean room, ADR 0081).
macro_rules! query {
    ($language:literal) => {
        include_str!(concat!("../queries/", $language, "/highlights.scm"))
    };
    ($language:literal, $kind:literal) => {
        include_str!(concat!("../queries/", $language, "/", $kind, ".scm"))
    };
}

const ECMASCRIPT_INDENTS: &str = query!("ecmascript", "indents");

/// Dialects extend their base language's queries, which therefore come first.
#[rustfmt::skip]
pub const INDENTS: &[Indents] = &[
    Indents::new(RUST, &[query!("rust", "indents")]),
    Indents::new(C, &[query!("c", "indents")]),
    Indents::new(CPP, &[query!("c", "indents"), query!("cpp", "indents")]),
    Indents::new(JAVASCRIPT, &[ECMASCRIPT_INDENTS, query!("jsx", "indents")]),
    Indents::new(JSX, &[ECMASCRIPT_INDENTS, query!("jsx", "indents")]),
    Indents::new(TYPESCRIPT, &[ECMASCRIPT_INDENTS, query!("typescript", "indents")]),
    Indents::new(TSX, &[ECMASCRIPT_INDENTS, query!("jsx", "indents"), query!("typescript", "indents")]),
    Indents::new(PYTHON, &[query!("python", "indents")]).only_blank_lines(),
    Indents::new(GO, &[query!("go", "indents")]),
    Indents::new(JAVA, &[query!("java", "indents")]),
    Indents::new(RUBY, &[query!("ruby", "indents")]),
    Indents::new(LUA, &[query!("lua", "indents")]),
    Indents::new(BASH, &[query!("bash", "indents")]),
    Indents::new(YAML, &[query!("yaml", "indents")]).only_blank_lines(),
    Indents::new(JSON, &[query!("json", "indents")]),
    Indents::new(HTML, &[query!("html", "indents")]),
    Indents::new(CSS, &[query!("css", "indents")]),
];

const ECMASCRIPT_LOCALS: &str = query!("ecmascript", "locals");

/// Dialects extend their base language's queries, which therefore come first.
#[rustfmt::skip]
pub const LOCALS: &[Locals] = &[
    Locals { mode: RUST, queries: &[query!("rust", "locals")] },
    Locals { mode: C, queries: &[query!("c", "locals")] },
    Locals { mode: CPP, queries: &[query!("c", "locals"), query!("cpp", "locals")] },
    Locals { mode: JAVASCRIPT, queries: &[ECMASCRIPT_LOCALS, query!("javascript", "locals")] },
    Locals { mode: JSX, queries: &[ECMASCRIPT_LOCALS, query!("javascript", "locals")] },
    Locals { mode: TYPESCRIPT, queries: &[ECMASCRIPT_LOCALS, query!("typescript", "locals")] },
    Locals { mode: TSX, queries: &[ECMASCRIPT_LOCALS, query!("typescript", "locals")] },
    Locals { mode: PYTHON, queries: &[query!("python", "locals")] },
    Locals { mode: GO, queries: &[query!("go", "locals")] },
    Locals { mode: JAVA, queries: &[query!("java", "locals")] },
    Locals { mode: RUBY, queries: &[query!("ruby", "locals")] },
    Locals { mode: LUA, queries: &[query!("lua", "locals")] },
];

const ECMASCRIPT: &str = query!("ecmascript");

/// Dialects extend their base language's queries, which therefore come first.
pub const GRAMMARS: &[Grammar] = &[
    Grammar {
        mode: RUST,
        language: Built(tree_sitter_rust::LANGUAGE),
        highlights: &[query!("rust"), query!("rust", "text")],
    },
    Grammar { mode: C, language: Built(tree_sitter_c::LANGUAGE), highlights: &[query!("c")] },
    Grammar {
        mode: CPP,
        language: Built(tree_sitter_cpp::LANGUAGE),
        highlights: &[query!("c"), query!("cpp")],
    },
    Grammar {
        mode: TYPESCRIPT,
        language: Built(tree_sitter_typescript::LANGUAGE_TYPESCRIPT),
        highlights: &[ECMASCRIPT, query!("typescript")],
    },
    Grammar {
        mode: TSX,
        language: Built(tree_sitter_typescript::LANGUAGE_TSX),
        highlights: &[ECMASCRIPT, query!("jsx"), query!("typescript")],
    },
    // React projects write JSX in plain `.js` files too.
    Grammar {
        mode: JAVASCRIPT,
        language: Built(tree_sitter_javascript::LANGUAGE),
        highlights: &[ECMASCRIPT, query!("javascript"), query!("jsx")],
    },
    Grammar {
        mode: JSX,
        language: Built(tree_sitter_javascript::LANGUAGE),
        highlights: &[ECMASCRIPT, query!("javascript"), query!("jsx")],
    },
    Grammar {
        mode: HTML,
        language: Built(tree_sitter_html::LANGUAGE),
        highlights: &[query!("html")],
    },
    Grammar { mode: CSS, language: Built(tree_sitter_css::LANGUAGE), highlights: &[query!("css")] },
    Grammar {
        mode: JSON,
        language: Built(tree_sitter_json::LANGUAGE),
        highlights: &[query!("json")],
    },
    Grammar {
        mode: TOML,
        language: Built(tree_sitter_toml_ng::LANGUAGE),
        highlights: &[query!("toml")],
    },
    Grammar {
        mode: YAML,
        language: Built(tree_sitter_yaml::LANGUAGE),
        highlights: &[query!("yaml")],
    },
    Grammar {
        mode: XML,
        language: Built(tree_sitter_xml::LANGUAGE_XML),
        highlights: &[query!("xml")],
    },
    // Block structure only: inline markup is a second grammar, injected into each block.
    Grammar {
        mode: MARKDOWN,
        language: Built(tree_sitter_md::LANGUAGE),
        highlights: &[query!("markdown")],
    },
    Grammar {
        mode: BASH,
        language: Built(tree_sitter_bash::LANGUAGE),
        highlights: &[query!("bash")],
    },
    Grammar {
        mode: MAKE,
        language: Built(tree_sitter_make::LANGUAGE),
        highlights: &[query!("make")],
    },
    Grammar {
        mode: CMAKE,
        language: Built(tree_sitter_cmake::LANGUAGE),
        highlights: &[query!("cmake")],
    },
    Grammar {
        mode: DIFF,
        language: Built(tree_sitter_diff::LANGUAGE),
        highlights: &[query!("diff")],
    },
    Grammar {
        mode: SQL,
        language: Built(tree_sitter_sequel::LANGUAGE),
        highlights: &[query!("sql")],
    },
    Grammar {
        mode: PYTHON,
        language: Built(tree_sitter_python::LANGUAGE),
        highlights: &[query!("python")],
    },
    Grammar { mode: GO, language: Built(tree_sitter_go::LANGUAGE), highlights: &[query!("go")] },
    Grammar {
        mode: JAVA,
        language: Built(tree_sitter_java::LANGUAGE),
        highlights: &[query!("java")],
    },
    Grammar {
        mode: CSHARP,
        language: Built(tree_sitter_c_sharp::LANGUAGE),
        highlights: &[query!("csharp")],
    },
    Grammar {
        mode: SWIFT,
        language: Built(tree_sitter_swift::LANGUAGE),
        highlights: &[query!("swift")],
    },
    Grammar {
        mode: OBJC,
        language: Built(tree_sitter_objc::LANGUAGE),
        highlights: &[query!("c"), query!("objc")],
    },
    Grammar {
        mode: DART,
        language: Built(tree_sitter_dart::LANGUAGE),
        highlights: &[query!("dart")],
    },
    Grammar {
        mode: SCALA,
        language: Built(tree_sitter_scala::LANGUAGE),
        highlights: &[query!("scala")],
    },
    // The full grammar, with the HTML around `<?php` tags.
    Grammar {
        mode: PHP,
        language: Built(tree_sitter_php::LANGUAGE_PHP),
        highlights: &[query!("php")],
    },
    Grammar {
        mode: RUBY,
        language: Built(tree_sitter_ruby::LANGUAGE),
        highlights: &[query!("ruby")],
    },
    Grammar { mode: LUA, language: Built(tree_sitter_lua::LANGUAGE), highlights: &[query!("lua")] },
    Grammar { mode: ZIG, language: Built(tree_sitter_zig::LANGUAGE), highlights: &[query!("zig")] },
    Grammar {
        mode: HASKELL,
        language: Built(tree_sitter_haskell::LANGUAGE),
        highlights: &[query!("haskell")],
    },
    Grammar {
        mode: ELIXIR,
        language: Built(tree_sitter_elixir::LANGUAGE),
        highlights: &[query!("elixir")],
    },
    Grammar { mode: NIX, language: Built(tree_sitter_nix::LANGUAGE), highlights: &[query!("nix")] },
    Grammar {
        mode: KOTLIN,
        language: Built(tree_sitter_kotlin_ng::LANGUAGE),
        highlights: &[query!("kotlin")],
    },
    Grammar { mode: HCL, language: Built(tree_sitter_hcl::LANGUAGE), highlights: &[query!("hcl")] },
    Grammar {
        mode: GRAPHQL,
        language: Built(tree_sitter_graphql::LANGUAGE),
        highlights: &[query!("graphql")],
    },
    Grammar {
        mode: PROTOBUF,
        language: Built(tree_sitter_proto::LANGUAGE),
        highlights: &[query!("proto")],
    },
    Grammar { mode: SCSS, language: Built(SCSS_LANGUAGE), highlights: &[query!("scss")] },
    Grammar {
        mode: VUE,
        language: Built(tree_sitter_vue_next::LANGUAGE),
        highlights: &[query!("vue")],
    },
    Grammar {
        mode: SVELTE,
        language: Built(tree_sitter_svelte_ng::LANGUAGE),
        highlights: &[query!("svelte")],
    },
    Grammar {
        mode: GIT_COMMIT,
        language: Built(tree_sitter_gitcommit::LANGUAGE),
        highlights: &[query!("gitcommit")],
    },
    // Downloaded from remacs's own releases, built from `grammars.toml` (ADR 0087).
    Grammar {
        mode: GIT_REBASE,
        language: downloaded("git_rebase"),
        highlights: &[query!("git_rebase")],
    },
    Grammar {
        mode: GIT_IGNORE,
        language: downloaded("gitignore"),
        highlights: &[query!("gitignore")],
    },
    Grammar {
        mode: DOCKERFILE,
        language: downloaded("dockerfile"),
        highlights: &[query!("dockerfile")],
    },
    Grammar { mode: FISH, language: downloaded("fish"), highlights: &[query!("fish")] },
    // Languages only within others, painted over them: no mode of their own.
    Grammar {
        mode: MARKDOWN_INLINE,
        language: Built(tree_sitter_md::INLINE_LANGUAGE),
        highlights: &[query!("markdown_inline")],
    },
    Grammar {
        mode: REGEX,
        language: Built(tree_sitter_regex::LANGUAGE),
        highlights: &[query!("regex")],
    },
    Grammar {
        mode: JSDOC,
        language: Built(tree_sitter_jsdoc::LANGUAGE),
        highlights: &[query!("jsdoc")],
    },
    Grammar {
        mode: RUST_MACRO_ARGUMENTS,
        language: Built(tree_sitter_rust::LANGUAGE),
        highlights: &[query!("rust")],
    },
];

/// The grammars injected into other languages only, which no file opens in.
#[cfg(test)]
pub(crate) const INJECTED_ONLY: &[&str] = &[MARKDOWN_INLINE, REGEX, JSDOC, RUST_MACRO_ARGUMENTS];
pub(crate) const RUST_MACRO_ARGUMENTS: &str = "rust_macro_arguments";
const MARKDOWN_INLINE: &str = "markdown_inline";
const REGEX: &str = "regex";
const JSDOC: &str = "jsdoc";

/// The release of `grammars.toml` that the queries of downloaded grammars are written for;
/// a grammar's new revision, whose node types may differ, comes with a new release.
const GRAMMARS_RELEASE: &str = "1";

const fn downloaded(name: &'static str) -> Source {
    Source::Downloaded { name, release: GRAMMARS_RELEASE }
}

// tree-sitter-scss 1.0 gives only a `Language` of the tree-sitter it was built against;
// its parser's C function is all a `LanguageFn` needs, and linking the crate brings it.
use tree_sitter_scss as _;
unsafe extern "C" {
    fn tree_sitter_scss() -> *const ();
}
// SAFETY: `tree_sitter_scss` is the grammar's generated C function, taking nothing and
// returning its `TSLanguage`, as `from_raw` asks.
const SCSS_LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_scss) };
