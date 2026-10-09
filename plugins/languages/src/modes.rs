use remacs_core::{CommentSyntax, Mode};

pub(crate) const RUST: &str = "Rust";
pub(crate) const C: &str = "C";
pub(crate) const CPP: &str = "C++";
pub(crate) const TYPESCRIPT: &str = "TypeScript";
pub(crate) const TSX: &str = "TSX";
pub(crate) const JAVASCRIPT: &str = "JavaScript";
pub(crate) const JSX: &str = "JSX";
pub(crate) const HTML: &str = "HTML";
pub(crate) const CSS: &str = "CSS";
pub(crate) const JSON: &str = "JSON";
pub(crate) const TOML: &str = "TOML";
pub(crate) const YAML: &str = "YAML";
pub(crate) const XML: &str = "XML";
pub(crate) const MARKDOWN: &str = "Markdown";
pub(crate) const BASH: &str = "Bash";
pub(crate) const MAKE: &str = "Make";
pub(crate) const CMAKE: &str = "CMake";
pub(crate) const DIFF: &str = "Diff";
pub(crate) const SQL: &str = "SQL";
pub(crate) const PYTHON: &str = "Python";
pub(crate) const GO: &str = "Go";
pub(crate) const JAVA: &str = "Java";
pub(crate) const CSHARP: &str = "C#";
pub(crate) const SWIFT: &str = "Swift";
pub(crate) const OBJC: &str = "Objective-C";
pub(crate) const DART: &str = "Dart";
pub(crate) const SCALA: &str = "Scala";
pub(crate) const PHP: &str = "PHP";
pub(crate) const RUBY: &str = "Ruby";
pub(crate) const LUA: &str = "Lua";
pub(crate) const ZIG: &str = "Zig";
pub(crate) const HASKELL: &str = "Haskell";
pub(crate) const ELIXIR: &str = "Elixir";
pub(crate) const NIX: &str = "Nix";

/// For languages where `'` is no string quote: Rust lifetimes, Haskell primes.
const DOUBLE_QUOTED_PAIRS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"')];
const QUOTED_PAIRS: &[(char, char)] =
    &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\'')];
const BACKTICK_PAIRS: &[(char, char)] = &[
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('"', '"'),
    ('\'', '\''),
    ('`', '`'),
];
/// `<` pairs only where it opens tags: elsewhere it is also less-than.
const JSX_PAIRS: &[(char, char)] = &[
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('<', '>'),
    ('"', '"'),
    ('\'', '\''),
    ('`', '`'),
];
const HTML_PAIRS: &[(char, char)] = &[
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('<', '>'),
    ('"', '"'),
    ('\'', '\''),
];
/// Quotes and apostrophes in prose are rarely pairs.
const MARKDOWN_PAIRS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('`', '`')];

const LINE_COMMENT: CommentSyntax = CommentSyntax {
    start: "// ",
    end: "",
};
const HASH_COMMENT: CommentSyntax = CommentSyntax {
    start: "# ",
    end: "",
};
const DASH_COMMENT: CommentSyntax = CommentSyntax {
    start: "-- ",
    end: "",
};
const BLOCK_COMMENT: CommentSyntax = CommentSyntax {
    start: "/* ",
    end: " */",
};
const HTML_COMMENT: CommentSyntax = CommentSyntax {
    start: "<!-- ",
    end: " -->",
};
const JSX_COMMENT: CommentSyntax = CommentSyntax {
    start: "{/* ",
    end: " */}",
};

const fn mode(
    name: &'static str,
    extensions: &'static [&'static str],
    pairs: &'static [(char, char)],
    comment: CommentSyntax,
) -> Mode {
    Mode {
        name,
        extensions,
        file_names: &[],
        pairs,
        comment: Some(comment),
        markup_comment: None,
    }
}

/// A JavaScript dialect whose markup lines comment out as JSX children.
const fn with_jsx(mode: Mode) -> Mode {
    Mode {
        markup_comment: Some(JSX_COMMENT),
        ..mode
    }
}

const fn with_file_names(mode: Mode, file_names: &'static [&'static str]) -> Mode {
    Mode { file_names, ..mode }
}

pub(crate) const MODES: &[Mode] = &[
    mode(RUST, &["rs"], DOUBLE_QUOTED_PAIRS, LINE_COMMENT),
    mode(C, &["c", "h"], QUOTED_PAIRS, LINE_COMMENT),
    mode(
        CPP,
        &["cc", "cpp", "cxx", "hh", "hpp", "hxx"],
        QUOTED_PAIRS,
        LINE_COMMENT,
    ),
    mode(
        TYPESCRIPT,
        &["ts", "mts", "cts"],
        BACKTICK_PAIRS,
        LINE_COMMENT,
    ),
    with_jsx(mode(TSX, &["tsx"], JSX_PAIRS, LINE_COMMENT)),
    // React projects write JSX in plain `.js` files too.
    with_jsx(mode(
        JAVASCRIPT,
        &["js", "mjs", "cjs"],
        BACKTICK_PAIRS,
        LINE_COMMENT,
    )),
    with_jsx(mode(JSX, &["jsx"], JSX_PAIRS, LINE_COMMENT)),
    mode(HTML, &["html", "htm"], HTML_PAIRS, HTML_COMMENT),
    mode(CSS, &["css"], QUOTED_PAIRS, BLOCK_COMMENT),
    // Line comments for the JSON with comments of tsconfig.json and the like.
    with_file_names(
        mode(JSON, &["json", "jsonc"], DOUBLE_QUOTED_PAIRS, LINE_COMMENT),
        &[".prettierrc", ".babelrc"],
    ),
    with_file_names(
        mode(TOML, &["toml"], QUOTED_PAIRS, HASH_COMMENT),
        &["Cargo.lock"],
    ),
    mode(YAML, &["yaml", "yml"], QUOTED_PAIRS, HASH_COMMENT),
    mode(
        XML,
        &["xml", "svg", "plist", "xsd", "xsl", "xslt"],
        HTML_PAIRS,
        HTML_COMMENT,
    ),
    mode(MARKDOWN, &["md", "markdown"], MARKDOWN_PAIRS, HTML_COMMENT),
    with_file_names(
        mode(BASH, &["sh", "bash", "zsh"], BACKTICK_PAIRS, HASH_COMMENT),
        &[
            ".bashrc",
            ".bash_profile",
            ".profile",
            ".zshrc",
            ".zprofile",
            ".zshenv",
        ],
    ),
    with_file_names(
        mode(MAKE, &["mk", "make"], QUOTED_PAIRS, HASH_COMMENT),
        &["Makefile", "makefile", "GNUmakefile"],
    ),
    with_file_names(
        mode(CMAKE, &["cmake"], DOUBLE_QUOTED_PAIRS, HASH_COMMENT),
        &["CMakeLists.txt"],
    ),
    Mode {
        name: DIFF,
        extensions: &["diff", "patch"],
        file_names: &[],
        pairs: &[],
        comment: None,
        markup_comment: None,
    },
    mode(SQL, &["sql"], QUOTED_PAIRS, DASH_COMMENT),
    mode(PYTHON, &["py", "pyi"], QUOTED_PAIRS, HASH_COMMENT),
    mode(GO, &["go"], BACKTICK_PAIRS, LINE_COMMENT),
    mode(JAVA, &["java"], QUOTED_PAIRS, LINE_COMMENT),
    mode(CSHARP, &["cs"], QUOTED_PAIRS, LINE_COMMENT),
    mode(SWIFT, &["swift"], DOUBLE_QUOTED_PAIRS, LINE_COMMENT),
    mode(OBJC, &["m"], QUOTED_PAIRS, LINE_COMMENT),
    mode(DART, &["dart"], QUOTED_PAIRS, LINE_COMMENT),
    mode(SCALA, &["scala", "sc", "sbt"], QUOTED_PAIRS, LINE_COMMENT),
    mode(PHP, &["php"], QUOTED_PAIRS, LINE_COMMENT),
    with_file_names(
        mode(RUBY, &["rb", "rake", "gemspec"], QUOTED_PAIRS, HASH_COMMENT),
        &["Gemfile", "Rakefile", "Podfile", "Brewfile"],
    ),
    mode(LUA, &["lua"], QUOTED_PAIRS, DASH_COMMENT),
    mode(ZIG, &["zig", "zon"], QUOTED_PAIRS, LINE_COMMENT),
    mode(HASKELL, &["hs"], DOUBLE_QUOTED_PAIRS, DASH_COMMENT),
    mode(ELIXIR, &["ex", "exs"], QUOTED_PAIRS, HASH_COMMENT),
    mode(NIX, &["nix"], DOUBLE_QUOTED_PAIRS, HASH_COMMENT),
];

/// Modes whose files prettier formats.
pub const PRETTIER_MODES: &[&str] = &[TYPESCRIPT, TSX, JAVASCRIPT, JSX, HTML, CSS];
