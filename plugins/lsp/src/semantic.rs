//! Semantic tokens (ADR 0088): what a language server knows each name of a file to be,
//! painted over the faces tree-sitter gives it, in the faces of remacs's capture scheme.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use remacs_core::Face;
use remacs_text::Snapshot;
use serde_json::Value;

use crate::position::{Encoding, offset_in_line};

/// The names a server gives its token types and modifiers, by their numbers.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Legend {
    pub types: Vec<String>,
    pub modifiers: Vec<String>,
}

impl Legend {
    /// From the server's capabilities, if it gives a whole file's tokens.
    pub fn offered(capabilities: &Value) -> Option<Self> {
        let provider = &capabilities["semanticTokensProvider"];
        let full = &provider["full"];
        if !(full.as_bool() == Some(true) || full.is_object()) {
            return None;
        }
        let names = |key: &str| -> Vec<String> {
            provider["legend"][key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|name| Some(name.as_str()?.to_owned()))
                .collect()
        };
        Some(Self {
            types: names("tokenTypes"),
            modifiers: names("tokenModifiers"),
        })
    }
}

/// A token's type and modifiers, as numbers of the legend; modifiers a bit each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Token {
    pub kind: u32,
    pub modifiers: u32,
}

/// The tokens of `data`, five numbers each with lines and columns relative to the one
/// before, as bytes of `text`.
pub fn decode(data: &[u32], text: &Snapshot, encoding: Encoding) -> Vec<(Range<usize>, Token)> {
    let mut line = 0;
    let mut bounds = text.line_range(0);
    // Where the token before on the line starts, as a byte and as the server counts.
    let mut at = (bounds.as_ref().map_or(0, |line| line.start), 0);
    data.as_chunks::<5>()
        .0
        .iter()
        .filter_map(|&[line_delta, column_delta, length, kind, modifiers]| {
            if line_delta > 0 {
                line += line_delta;
                bounds = text.line_range(line as usize);
                at = (bounds.as_ref().map_or(0, |line| line.start), 0);
            }
            let line = bounds.clone()?;
            let column = at.1 + column_delta;
            let start = offset_in_line(text, line.clone(), at, column, encoding);
            let end = offset_in_line(text, line, (start, column), column + length, encoding);
            at = (start, column);
            let token = Token { kind, modifiers };
            (start < end).then_some((start..end, token))
        })
        .collect()
}

/// The capture of remacs's scheme each token type takes, `type.modifier` for a modifier
/// that changes it, the first of its modifiers that does; `""` paints nothing. The LSP's
/// own types, then rust-analyzer's and TypeScript's more. What tree-sitter tells by itself,
/// comments, strings, numbers, keywords, is left to it: a server's `comment` over a whole
/// doc comment, which rust-analyzer sends anyway, would hide the TODO marked in it.
pub(crate) const CAPTURES: &[(&str, &str)] = &[
    ("namespace", ""),
    ("type", "type"),
    ("class", "type"),
    ("enum", "type"),
    ("interface", "type"),
    ("struct", "type"),
    ("typeParameter", "type"),
    ("parameter", "variable"),
    ("variable", "variable"),
    ("variable.constant", "constant"),
    ("variable.static", "constant"),
    ("property", ""),
    ("enumMember", "constant"),
    ("event", ""),
    ("function", "function.call"),
    ("function.declaration", "function"),
    ("function.definition", "function"),
    ("method", "function.call"),
    ("method.declaration", "function"),
    ("method.definition", "function"),
    ("macro", "function.macro"),
    ("decorator", "attribute"),
    ("label", "label"),
    ("attribute", "attribute"),
    ("builtinAttribute", "attribute"),
    ("builtinType", "type.builtin"),
    ("const", "constant"),
    ("constParameter", "constant"),
    ("derive", "type"),
    ("deriveHelper", "attribute"),
    ("formatSpecifier", ""),
    ("generic", ""),
    ("lifetime", "label"),
    ("macroBang", "function.macro"),
    ("procMacro", "function.macro"),
    ("selfKeyword", "variable.builtin"),
    ("selfTypeKeyword", "type"),
    ("static", "constant"),
    ("toolModule", ""),
    ("typeAlias", "type"),
    ("union", "type"),
    ("unresolvedReference", ""),
];

/// How tokens are painted: `lsp.toml`'s `semantic-tokens` and its captures over the
/// built-in ones, with `syntax.toml`'s highlight level and whether variables are painted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Painting {
    pub enabled: bool,
    pub captures: BTreeMap<String, String>,
    pub level: u8,
    pub variables: bool,
}

impl Painting {
    fn face(&self, legend: &Legend, token: Token) -> Option<Face> {
        let kind = legend.types.get(token.kind as usize)?;
        let capture = (legend.modifiers.iter().enumerate())
            .filter(|&(bit, _)| bit < 32 && token.modifiers >> bit & 1 == 1)
            .find_map(|(_, modifier)| self.captures.get(&format!("{kind}.{modifier}")))
            .or_else(|| self.captures.get(kind))?;
        let (face, level) = remacs_syntax::face_for(capture)?;
        (level <= self.level && (face != Face::Variable || self.variables)).then_some(face)
    }

    /// The faces of `tokens`; a token without one leaves tree-sitter's.
    pub fn paint(
        &self,
        legend: &Legend,
        tokens: &[(Range<usize>, Token)],
    ) -> Vec<(Range<usize>, Face)> {
        let mut faces: HashMap<Token, Option<Face>> = HashMap::new();
        tokens
            .iter()
            .filter_map(|(range, token)| {
                let face = *faces
                    .entry(*token)
                    .or_insert_with(|| self.face(legend, *token));
                Some((range.clone(), face?))
            })
            .collect()
    }
}

/// The built-in captures with `own` over them.
pub(crate) fn captures(own: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut captures: BTreeMap<String, String> = CAPTURES
        .iter()
        .map(|&(kind, capture)| (kind.to_owned(), capture.to_owned()))
        .collect();
    captures.extend(own.clone());
    captures
}

#[cfg(test)]
mod tests {
    use remacs_text::Text;
    use serde_json::json;

    use super::*;

    fn legend() -> Legend {
        Legend::offered(&json!({ "semanticTokensProvider": {
            "full": { "delta": true },
            "legend": {
                "tokenTypes": ["function", "parameter", "property", "macro", "variable"],
                "tokenModifiers": ["declaration", "documentation", "constant"],
            },
        }}))
        .unwrap()
    }

    fn painting(level: u8, variables: bool) -> Painting {
        Painting {
            enabled: true,
            captures: captures(&BTreeMap::new()),
            level,
            variables,
        }
    }

    #[test]
    fn tokens_are_read_relative_to_the_one_before_and_painted_in_the_scheme() {
        // `é` is two bytes and one UTF-16 unit, so `go` starts at column 4, byte 5.
        let text = Text::from("fn é(x: u8) {\n  go(x);\n  M!();\n}\n").snapshot();
        let data = [
            0, 3, 1, 0, 0b001, // é, declared
            0, 2, 1, 1, 0, // x
            1, 2, 2, 0, 0, // go, a call
            0, 3, 1, 1, 0, // x
            1, 2, 1, 3, 0, // M, a macro
        ];
        let tokens = decode(&data, &text, Encoding::Utf16);
        let shown = |painting: &Painting| -> Vec<(String, Face)> {
            painting
                .paint(&legend(), &tokens)
                .into_iter()
                .map(|(range, face)| {
                    (
                        String::from_utf8(text.bytes_in(range).to_vec()).unwrap(),
                        face,
                    )
                })
                .collect()
        };
        assert_eq!(
            shown(&painting(4, true)),
            [
                ("é".into(), Face::Function),
                ("x".into(), Face::Variable),
                ("go".into(), Face::Function),
                ("x".into(), Face::Variable),
                ("M".into(), Face::Builtin),
            ]
        );
        // Calls and variables wait for level 4, macros for 3; variables can be left.
        assert_eq!(
            shown(&painting(3, true)),
            [("é".into(), Face::Function), ("M".into(), Face::Builtin)]
        );
        assert_eq!(
            shown(&painting(4, false)),
            [
                ("é".into(), Face::Function),
                ("go".into(), Face::Function),
                ("M".into(), Face::Builtin),
            ]
        );
    }

    #[test]
    fn a_modifier_changes_the_capture_and_own_captures_win() {
        let legend = legend();
        let constant = Token {
            kind: 4,
            modifiers: 0b100,
        };
        let property = Token {
            kind: 2,
            modifiers: 0,
        };
        let mut painting = painting(4, true);
        assert_eq!(painting.face(&legend, constant), Some(Face::Constant));
        assert_eq!(painting.face(&legend, property), None);
        let own = BTreeMap::from([("property".to_owned(), "constant".to_owned())]);
        painting.captures = captures(&own);
        assert_eq!(painting.face(&legend, property), Some(Face::Constant));
        // A number the legend lacks is no token.
        let unknown = Token {
            kind: 9,
            modifiers: 0,
        };
        assert_eq!(painting.face(&legend, unknown), None);
    }

    #[test]
    fn the_default_file_lists_every_built_in_capture() {
        let file = include_str!("../../../config/defaults/lsp.toml");
        let listed: Vec<&str> = file
            .lines()
            .skip_while(|line| *line != "[semantic-token-captures]")
            .skip(1)
            .take_while(|line| line.starts_with("# ") && line.contains(" = \""))
            .collect();
        let built_in: Vec<String> = CAPTURES
            .iter()
            .map(|(kind, capture)| match kind.contains('.') {
                true => format!("# \"{kind}\" = \"{capture}\""),
                false => format!("# {kind} = \"{capture}\""),
            })
            .collect();
        assert_eq!(listed, built_in);
        for (_, capture) in CAPTURES {
            let known = capture.is_empty() || remacs_syntax::face_for(capture).is_some();
            assert!(known, "{capture}");
        }
    }

    #[test]
    fn servers_that_give_no_whole_file_offer_nothing() {
        assert_eq!(Legend::offered(&json!({})), None);
        let ranges_only = json!({ "semanticTokensProvider": { "range": true, "legend": {} } });
        assert_eq!(Legend::offered(&ranges_only), None);
    }
}
