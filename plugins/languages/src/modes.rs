use std::sync::LazyLock;

use remacs_core::{CommentSyntax, Mode};
use serde::Deserialize;

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
pub(crate) const KOTLIN: &str = "Kotlin";
pub(crate) const HCL: &str = "HCL";
pub(crate) const GRAPHQL: &str = "GraphQL";
pub(crate) const PROTOBUF: &str = "Protobuf";
pub(crate) const SCSS: &str = "SCSS";
pub(crate) const VUE: &str = "Vue";
pub(crate) const SVELTE: &str = "Svelte";
pub(crate) const GIT_COMMIT: &str = "Git Commit";

/// Modes whose files prettier formats.
pub const PRETTIER_MODES: &[&str] = &[TYPESCRIPT, TSX, JAVASCRIPT, JSX, HTML, CSS];

/// `languages.toml`, read once and kept for the program's life, as modes are `'static`.
pub(crate) static MODES: LazyLock<Vec<Mode>> = LazyLock::new(|| {
    let catalog: Catalog = toml::from_str(include_str!("../languages.toml"))
        .unwrap_or_else(|error| panic!("languages.toml: {error}"));
    catalog
        .languages
        .into_iter()
        .map(Language::into_mode)
        .collect()
});

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    languages: Vec<Language>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
struct Language {
    name: String,
    extensions: Vec<String>,
    file_names: Vec<String>,
    interpreters: Vec<String>,
    aliases: Vec<String>,
    first_lines: Vec<String>,
    pairs: Vec<String>,
    comment: Option<String>,
    comment_end: String,
    markup_comment: Option<String>,
    markup_comment_end: String,
}

impl Language {
    fn into_mode(self) -> Mode {
        let pairs: Vec<(char, char)> = (self.pairs.iter())
            .map(|pair| match pair.chars().collect::<Vec<_>>()[..] {
                [open, close] => (open, close),
                _ => panic!(
                    "languages.toml: {}: a pair is two characters: {pair}",
                    self.name
                ),
            })
            .collect();
        Mode {
            name: self.name.leak(),
            extensions: leak(self.extensions),
            file_names: leak(self.file_names),
            interpreters: leak(self.interpreters),
            aliases: leak(self.aliases),
            first_lines: leak(self.first_lines),
            pairs: pairs.leak(),
            comment: comment(self.comment, self.comment_end),
            markup_comment: comment(self.markup_comment, self.markup_comment_end),
        }
    }
}

fn leak(strings: Vec<String>) -> &'static [&'static str] {
    let strings: Vec<&'static str> = strings.into_iter().map(|string| &*string.leak()).collect();
    strings.leak()
}

fn comment(start: Option<String>, end: String) -> Option<CommentSyntax> {
    Some(CommentSyntax {
        start: start?.leak(),
        end: end.leak(),
    })
}
