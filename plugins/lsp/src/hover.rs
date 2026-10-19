use lsp_types::{Hover, HoverContents, MarkedString, MarkupKind};
use remacs_core::{Code, DocBlock, Documentation};
use remacs_documentation::Answer;
use remacs_text::Snapshot;

use crate::markdown::{self, CodeModes};
use crate::position::{Encoding, from_lsp};

/// Declaration keywords and the kind of symbol they declare.
const KINDS: &[(&str, &str)] = &[
    ("fn", "function"),
    ("function", "function"),
    ("struct", "struct"),
    ("enum", "enum"),
    ("union", "union"),
    ("trait", "trait"),
    ("interface", "interface"),
    ("class", "class"),
    ("type", "type"),
    ("const", "constant"),
    ("static", "static"),
    ("mod", "module"),
    ("module", "module"),
    ("namespace", "namespace"),
    ("macro_rules!", "macro"),
    ("let", "variable"),
    ("var", "variable"),
];
/// Words that may come before `fn` in a Rust declaration.
const FN_QUALIFIERS: &[&str] = &["const", "async", "unsafe", "extern", "safe"];

/// The documentation at a position of `text`, the text the server had when asked.
pub(crate) fn answer(
    hover: Option<Hover>,
    text: &Snapshot,
    encoding: Encoding,
    modes: CodeModes,
) -> Option<Answer> {
    let hover = hover?;
    let blocks = match hover.contents {
        HoverContents::Markup(content) if content.kind == MarkupKind::PlainText => {
            markdown::plain_blocks(&content.value)
        }
        HoverContents::Markup(content) => markdown::blocks(&content.value, modes),
        HoverContents::Scalar(marked) => markdown::blocks(&to_markdown(marked), modes),
        HoverContents::Array(marked) => markdown::blocks(
            &marked
                .into_iter()
                .map(to_markdown)
                .collect::<Vec<_>>()
                .join("\n\n"),
            modes,
        ),
    };
    let range = hover
        .range
        .map(|range| from_lsp(text, range.start, encoding)..from_lsp(text, range.end, encoding));
    Some(Answer {
        range: range.filter(|range| !range.is_empty()),
        documentation: documentation(blocks),
    })
}

fn to_markdown(marked: MarkedString) -> String {
    match marked {
        MarkedString::String(markdown) => markdown,
        MarkedString::LanguageString(code) => format!("```{}\n{}\n```", code.language, code.value),
    }
}

/// Most servers lead with the declaration in a code block: rust-analyzer with the module
/// in another one before it, TypeScript with the kind in it, `(method) …`. clangd leads
/// with a heading, `function `strlen``, and ends with the declaration.
fn documentation(mut blocks: Vec<DocBlock>) -> Documentation {
    let mut container = Vec::new();
    let mut kind = String::new();
    let mut signature = take_code(&mut blocks, 0);
    if signature
        .as_ref()
        .is_some_and(|code| !code.text.contains('\n'))
        && let Some(declaration) = take_code(&mut blocks, 0)
    {
        container.extend(signature.replace(declaration).map(|module| module.text));
    } else if signature.is_none()
        && let Some(DocBlock::Heading(heading)) = blocks.first()
        && let Some((named, _)) = heading.split_once(" `")
    {
        kind = named.to_owned();
        blocks.remove(0);
        let last = blocks.len().saturating_sub(1);
        signature = take_code(&mut blocks, last);
    }
    let signature = signature.map(|Code { text, mode, .. }| {
        let (context, mut declaration) = split_context(&text);
        container.extend(context);
        if let Some((named, rest)) = declaration
            .strip_prefix('(')
            .and_then(|rest| rest.split_once(") "))
        {
            kind = named.to_owned();
            declaration = rest.to_owned();
        } else if kind.is_empty() {
            kind = kind_of(&declaration).unwrap_or_default().to_owned();
        }
        Code::new(declaration, mode)
    });
    Documentation {
        container: container.join(" · "),
        kind,
        signature,
        blocks,
    }
}

fn take_code(blocks: &mut Vec<DocBlock>, index: usize) -> Option<Code> {
    let DocBlock::Code(_) = blocks.get(index)? else {
        return None;
    };
    match blocks.remove(index) {
        DocBlock::Code(code) => Some(code),
        _ => unreachable!("the block is code"),
    }
}

/// Lines before a declaration that say where it is: clangd's `// In string.h`,
/// rust-analyzer's `impl<T> [T]`.
fn split_context(declaration: &str) -> (Vec<String>, String) {
    let lines: Vec<_> = declaration.lines().collect();
    let count = lines[..lines.len().saturating_sub(1)]
        .iter()
        .take_while(|line| line.starts_with("// In ") || line.starts_with("impl"))
        .count();
    let context = lines[..count]
        .iter()
        .map(|line| line.strip_prefix("// In ").unwrap_or(line).to_owned())
        .collect();
    (context, lines[count..].join("\n"))
}

/// What a declaration declares, by its first keyword; a function taking `self` is a
/// method and `name: Type` a field.
fn kind_of(declaration: &str) -> Option<&'static str> {
    let line = declaration.lines().next()?;
    let words: Vec<_> = line.split_whitespace().take(6).collect();
    for (index, word) in words.iter().enumerate() {
        let qualifies_fn = FN_QUALIFIERS.contains(word)
            && words
                .get(index + 1)
                .is_some_and(|next| *next == "fn" || FN_QUALIFIERS.contains(next));
        if qualifies_fn {
            continue;
        }
        if let Some(&(_, kind)) = KINDS.iter().find(|(keyword, _)| keyword == word) {
            let takes_self =
                line.contains("(self") || line.contains("self,") || line.contains("self)");
            return Some(if kind == "function" && takes_self {
                "method"
            } else {
                kind
            });
        }
    }
    let name = words.iter().find(|word| !word.starts_with("pub"))?;
    (name.ends_with(':') && name.len() > 1).then_some("field")
}

#[cfg(test)]
mod tests {
    use remacs_text::Text;
    use serde_json::json;

    use super::*;

    const LANGUAGES: &[(&str, &str)] = &[
        ("Rust", "rust"),
        ("C", "c"),
        ("C++", "cpp"),
        ("TypeScript", "typescript"),
    ];

    fn convert(hover: serde_json::Value, source: &str, document: &'static str) -> Answer {
        let hover = serde_json::from_value(hover).unwrap();
        let modes = CodeModes {
            document,
            languages: LANGUAGES,
        };
        answer(
            Some(hover),
            &Text::from(source).snapshot(),
            Encoding::Utf16,
            modes,
        )
        .unwrap()
    }

    #[test]
    fn rust_analyzer_leads_with_the_module() {
        let value = "```rust\ncore::slice\n```\n\n```rust\nimpl<T> [T]\npub const fn copy_from_slice(&mut self, src: &[T])\nwhere\n    T: Copy,\n```\n\n---\n\nCopies all elements.\n\n# Panics\n\nIf the lengths differ.";
        let hover = json!({
            "contents": { "kind": "markdown", "value": value },
            "range": { "start": { "line": 0, "character": 5 }, "end": { "line": 0, "character": 9 } }
        });
        let answer = convert(hover, "self.copy(s)", "Rust");
        assert_eq!(answer.range, Some(5..9));
        assert_eq!(
            answer.documentation,
            Documentation {
                container: "core::slice · impl<T> [T]".into(),
                kind: "method".into(),
                signature: Some(Code::new(
                    "pub const fn copy_from_slice(&mut self, src: &[T])\nwhere\n    T: Copy,",
                    Some("Rust")
                )),
                blocks: vec![
                    DocBlock::Text("Copies all elements.".into()),
                    DocBlock::Heading("Panics".into()),
                    DocBlock::Text("If the lengths differ.".into()),
                ],
            }
        );
    }

    #[test]
    fn kinds_come_from_the_declaration() {
        let typescript = json!({ "contents": [{ "language": "typescript", "value": "(method) String.toUpperCase(): string" }, "Converts to uppercase."] });
        let documentation = convert(typescript, "", "TSX").documentation;
        assert_eq!(documentation.kind, "method");
        assert_eq!(
            documentation.signature,
            Some(Code::new(
                "String.toUpperCase(): string",
                Some("TypeScript")
            ))
        );
        assert_eq!(documentation.container, "");
        assert_eq!(kind_of("pub start: usize"), Some("field"));
        assert_eq!(kind_of("pub(crate) const MAX: usize = 2"), Some("constant"));
        assert_eq!(kind_of("pub async unsafe fn run()"), Some("function"));
        assert_eq!(kind_of("pub fn len(&self) -> usize"), Some("method"));
        assert_eq!(kind_of("size_t strlen(const char *s)"), None);
    }

    #[test]
    fn clangd_ends_with_the_declaration() {
        let value = "### function `strlen`  \nprovided by `<string.h>`  \n\n---\n→ `unsigned long`  \nParameters:  \n- `const char * __s`\n\n---\n```cpp\n// In string.h\nunsigned long strlen(const char *__s)\n```";
        let hover = json!({ "contents": { "kind": "markdown", "value": value } });
        assert_eq!(
            convert(hover, "", "C").documentation,
            Documentation {
                container: "string.h".into(),
                kind: "function".into(),
                signature: Some(Code::new(
                    "unsigned long strlen(const char *__s)",
                    Some("C++")
                )),
                blocks: vec![
                    DocBlock::Text("provided by `<string.h>`".into()),
                    DocBlock::Text("→ `unsigned long`".into()),
                    DocBlock::Text("Parameters:".into()),
                    DocBlock::Text("• `const char * __s`".into()),
                ],
            }
        );
    }
}
