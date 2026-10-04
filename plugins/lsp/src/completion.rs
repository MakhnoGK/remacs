use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit,
    CompletionTriggerKind, InsertTextFormat, MarkupKind,
};
use remacs_completion::{Item, List, Trigger, is_word_char};
use remacs_core::{Code, Documentation, SymbolKind};
use remacs_text::Snapshot;
use serde_json::Value;

use crate::markdown::{self, CodeModes};
use crate::position::{Encoding, from_lsp};
use crate::snippet;

/// What a server's `completionProvider` offers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CompletionSupport {
    /// Characters after which the server completes, such as `.`.
    trigger_characters: Vec<char>,
    resolves: bool,
}

impl CompletionSupport {
    pub fn from_capabilities(capabilities: &Value) -> Option<Self> {
        let provider = capabilities
            .get("completionProvider")
            .filter(|provider| provider.is_object())?;
        let trigger_characters = provider["triggerCharacters"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|trigger| {
                let mut chars = trigger.as_str()?.chars();
                chars.next().filter(|_| chars.next().is_none())
            })
            .collect();
        Some(Self {
            trigger_characters,
            resolves: provider["resolveProvider"] == true,
        })
    }

    pub fn resolves(&self) -> bool {
        self.resolves
    }

    /// The request's context; `None` when the server does not complete after the typed
    /// character.
    pub fn context(&self, trigger: Trigger) -> Option<CompletionContext> {
        let (trigger_kind, trigger_character) = match trigger {
            Trigger::Typed(typed) if !is_word_char(typed) => {
                if !self.trigger_characters.contains(&typed) {
                    return None;
                }
                (
                    CompletionTriggerKind::TRIGGER_CHARACTER,
                    Some(typed.to_string()),
                )
            }
            Trigger::Explicit | Trigger::Typed(_) => (CompletionTriggerKind::INVOKED, None),
            Trigger::Incomplete => (
                CompletionTriggerKind::TRIGGER_FOR_INCOMPLETE_COMPLETIONS,
                None,
            ),
        };
        Some(CompletionContext {
            trigger_kind,
            trigger_character,
        })
    }
}

/// The text a list was asked for: the server's positions refer to it, and items without
/// an edit replace the word from `start`.
#[derive(Clone)]
pub(crate) struct Origin {
    pub text: Snapshot,
    pub start: usize,
    pub encoding: Encoding,
    pub modes: CodeModes,
}

/// The items in the server's order, converted and as sent, the latter for resolving.
pub(crate) fn list(
    response: Option<CompletionResponse>,
    origin: &Origin,
) -> (List, Vec<CompletionItem>) {
    let (mut items, incomplete) = match response {
        None => (Vec::new(), false),
        Some(CompletionResponse::Array(items)) => (items, false),
        Some(CompletionResponse::List(list)) => (list.items, list.is_incomplete),
    };
    items.sort_by(|a, b| sort_text(a).cmp(sort_text(b)));
    let converted = items.iter().map(|item| convert(item, origin)).collect();
    (
        List {
            items: converted,
            incomplete,
        },
        items,
    )
}

fn sort_text(item: &CompletionItem) -> &str {
    item.sort_text.as_deref().unwrap_or(&item.label)
}

pub(crate) fn convert(item: &CompletionItem, origin: &Origin) -> Item {
    let offset = |position| from_lsp(&origin.text, position, origin.encoding);
    let (start, text) = match &item.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => (offset(edit.range.start), &edit.new_text),
        Some(CompletionTextEdit::InsertAndReplace(edit)) => {
            (offset(edit.insert.start), &edit.new_text)
        }
        None => (
            origin.start,
            item.insert_text.as_ref().unwrap_or(&item.label),
        ),
    };
    let (insert, cursor) = if item.insert_text_format == Some(InsertTextFormat::SNIPPET) {
        snippet::expand(text)
    } else {
        (text.clone(), None)
    };
    let kind = item.kind.and_then(symbol_kind);
    let detail = item
        .label_details
        .as_ref()
        .and_then(|details| details.description.as_deref())
        .or(item.detail.as_deref())
        .and_then(|detail| detail.lines().next())
        .unwrap_or_default()
        .trim()
        .to_owned();
    Item {
        // clangd pads labels to line them up with its `•` for items that add an include.
        label: item.label.trim().to_owned(),
        kind,
        detail,
        filter_text: item
            .filter_text
            .clone()
            .unwrap_or_else(|| item.label.clone()),
        start,
        insert,
        cursor,
        extra_edits: item
            .additional_text_edits
            .iter()
            .flatten()
            .map(|edit| {
                let range = offset(edit.range.start)..offset(edit.range.end);
                (range, edit.new_text.clone())
            })
            .collect(),
        documentation: documentation(item, kind, origin.modes),
    }
}

/// `None` while the server has said nothing about the item, so it is worth resolving.
fn documentation(
    item: &CompletionItem,
    kind: Option<SymbolKind>,
    modes: CodeModes,
) -> Option<Documentation> {
    let signature = item
        .detail
        .as_deref()
        .map(str::trim)
        .filter(|detail| !detail.is_empty())
        .map(|detail| Code::new(detail, Some(modes.document)));
    let blocks = match &item.documentation {
        None if signature.is_none() => return None,
        None => Vec::new(),
        Some(lsp_types::Documentation::String(text)) => markdown::plain_blocks(text),
        Some(lsp_types::Documentation::MarkupContent(content)) => match content.kind {
            MarkupKind::Markdown => markdown::blocks(&content.value, modes),
            MarkupKind::PlainText => markdown::plain_blocks(&content.value),
        },
    };
    Some(Documentation {
        kind: kind.map_or("", SymbolKind::name).to_owned(),
        signature,
        blocks,
        ..Documentation::default()
    })
}

fn symbol_kind(kind: CompletionItemKind) -> Option<SymbolKind> {
    Some(match kind {
        CompletionItemKind::METHOD => SymbolKind::Method,
        CompletionItemKind::FUNCTION
        | CompletionItemKind::CONSTRUCTOR
        | CompletionItemKind::OPERATOR => SymbolKind::Function,
        CompletionItemKind::FIELD | CompletionItemKind::PROPERTY | CompletionItemKind::EVENT => {
            SymbolKind::Field
        }
        CompletionItemKind::VARIABLE | CompletionItemKind::REFERENCE => SymbolKind::Variable,
        CompletionItemKind::CONSTANT | CompletionItemKind::VALUE => SymbolKind::Constant,
        CompletionItemKind::ENUM_MEMBER => SymbolKind::EnumMember,
        CompletionItemKind::CLASS
        | CompletionItemKind::STRUCT
        | CompletionItemKind::ENUM
        | CompletionItemKind::UNIT
        | CompletionItemKind::TYPE_PARAMETER => SymbolKind::Type,
        CompletionItemKind::INTERFACE => SymbolKind::Interface,
        CompletionItemKind::MODULE | CompletionItemKind::FOLDER | CompletionItemKind::FILE => {
            SymbolKind::Module
        }
        CompletionItemKind::KEYWORD => SymbolKind::Keyword,
        CompletionItemKind::SNIPPET => SymbolKind::Snippet,
        CompletionItemKind::TEXT | CompletionItemKind::COLOR => SymbolKind::Text,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use remacs_text::Text;
    use serde_json::json;

    use super::*;

    fn origin(source: &str, start: usize) -> Origin {
        Origin {
            text: Text::from(source).snapshot(),
            start,
            encoding: Encoding::Utf16,
            modes: CodeModes {
                document: "Rust",
                languages: &[("Rust", "rust")],
            },
        }
    }

    #[test]
    fn items_are_sorted_and_converted() {
        let source = "use a;\nlet ж = self.gap.st";
        let start = source.len() - 2;
        let response = json!({
            "isIncomplete": true,
            "items": [
                {
                    "label": "step_by(…)", "kind": 2, "sortText": "b", "filterText": "step_by",
                    "insertTextFormat": 2,
                    "textEdit": { "newText": "step_by(${1:step})$0",
                        "insert": { "start": { "line": 1, "character": 17 }, "end": { "line": 1, "character": 19 } },
                        "replace": { "start": { "line": 1, "character": 17 }, "end": { "line": 1, "character": 19 } } },
                    "labelDetails": { "description": "StepBy<Range<usize>>" },
                    "detail": "fn(self, usize) -> StepBy<Self>",
                    "additionalTextEdits": [{ "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } }, "newText": "use b;\n" }]
                },
                {
                    "label": "start", "kind": 5, "sortText": "a", "detail": "usize",
                    "documentation": { "kind": "markdown", "value": "The lower bound." }
                },
                { "label": "is_sorted", "kind": 2 }
            ]
        });
        let response = serde_json::from_value(response).unwrap();
        let (list, raw) = list(Some(response), &origin(source, start));
        assert!(list.incomplete);
        assert_eq!(raw[2].label, "is_sorted");
        let labels: Vec<_> = list.items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["start", "step_by(…)", "is_sorted"]);

        assert_eq!(list.items[2].documentation, None);
        let field = &list.items[0];
        assert_eq!((field.start, field.insert.as_str()), (start, "start"));
        assert_eq!(
            field.documentation,
            Some(Documentation {
                kind: "field".into(),
                signature: Some(Code::new("usize", Some("Rust"))),
                blocks: vec![remacs_core::DocBlock::Text("The lower bound.".into())],
                ..Documentation::default()
            })
        );
        let method = &list.items[1];
        assert_eq!(method.kind, Some(SymbolKind::Method));
        assert_eq!(method.detail, "StepBy<Range<usize>>");
        assert_eq!(method.filter_text, "step_by");
        assert_eq!(
            (method.start, method.insert.as_str(), method.cursor),
            (start, "step_by(step)", Some(8))
        );
        assert_eq!(method.extra_edits, [(0..0, "use b;\n".into())]);
    }

    #[test]
    fn typed_characters_trigger_only_where_the_server_completes() {
        let capabilities = json!({ "completionProvider": { "triggerCharacters": [".", "::"], "resolveProvider": true } });
        let support = CompletionSupport::from_capabilities(&capabilities).unwrap();
        assert!(support.resolves());
        let kind = |trigger| support.context(trigger).map(|context| context.trigger_kind);
        assert_eq!(
            kind(Trigger::Typed('a')),
            Some(CompletionTriggerKind::INVOKED)
        );
        assert_eq!(
            support
                .context(Trigger::Typed('.'))
                .unwrap()
                .trigger_character
                .as_deref(),
            Some(".")
        );
        assert_eq!(kind(Trigger::Typed(':')), None);
        assert_eq!(
            kind(Trigger::Incomplete),
            Some(CompletionTriggerKind::TRIGGER_FOR_INCOMPLETE_COMPLETIONS)
        );
        assert_eq!(CompletionSupport::from_capabilities(&json!({})), None);
    }
}
