use std::ops::Range;

use lsp_types::{
    ParameterInformation, ParameterLabel, SignatureHelp, SignatureHelpContext,
    SignatureHelpTriggerKind,
};
use remacs_core::Code;
use remacs_documentation::{SignatureTrigger, Signatures};
use serde_json::Value;

/// What a server's `signatureHelpProvider` offers: the characters it shows signatures
/// after, such as `(`, and those it updates them after while shown, such as `)`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SignatureSupport {
    triggers: Vec<char>,
    retriggers: Vec<char>,
}

impl SignatureSupport {
    pub fn from_capabilities(capabilities: &Value) -> Option<Self> {
        let provider = capabilities
            .get("signatureHelpProvider")
            .filter(|provider| provider.is_object())?;
        let characters = |key: &str| {
            let listed = provider[key].as_array().into_iter().flatten();
            listed
                .filter_map(|character| character.as_str()?.chars().next())
                .collect()
        };
        Some(Self {
            triggers: characters("triggerCharacters"),
            retriggers: characters("retriggerCharacters"),
        })
    }

    /// The request's context; `None` when a character typed does not call for signatures.
    pub fn context(&self, trigger: SignatureTrigger) -> Option<SignatureHelpContext> {
        let (trigger_kind, character, is_retrigger) = match trigger {
            SignatureTrigger::Explicit => (SignatureHelpTriggerKind::INVOKED, None, false),
            SignatureTrigger::Typed(typed) if self.triggers.contains(&typed) => (
                SignatureHelpTriggerKind::TRIGGER_CHARACTER,
                Some(typed),
                false,
            ),
            SignatureTrigger::Typed(_) => return None,
            SignatureTrigger::Update(Some(typed))
                if self.triggers.contains(&typed) || self.retriggers.contains(&typed) =>
            {
                (
                    SignatureHelpTriggerKind::TRIGGER_CHARACTER,
                    Some(typed),
                    true,
                )
            }
            SignatureTrigger::Update(_) => (SignatureHelpTriggerKind::CONTENT_CHANGE, None, true),
        };
        Some(SignatureHelpContext {
            trigger_kind,
            trigger_character: character.map(String::from),
            is_retrigger,
            active_signature_help: None,
        })
    }
}

/// The signatures written in `mode`, each with its active parameter as the emphasis.
pub(crate) fn signatures(help: Option<SignatureHelp>, mode: &'static str) -> Option<Signatures> {
    let help = help.filter(|help| !help.signatures.is_empty())?;
    let active = help.active_signature.unwrap_or(0) as usize;
    let list: Vec<_> = help
        .signatures
        .into_iter()
        .map(|signature| {
            let parameter = signature.active_parameter.or(help.active_parameter);
            let emphasis = parameter.and_then(|index| {
                let parameters = signature.parameters.as_deref()?;
                parameter_range(&signature.label, parameters, index as usize)
            });
            Code {
                emphasis,
                ..Code::new(signature.label, Some(mode))
            }
        })
        .collect();
    Some(Signatures {
        active: active.min(list.len() - 1),
        list,
    })
}

/// Where parameter `index` is in its signature's label: offsets in UTF-16 code units, or
/// its text, looked for past the name and the parameters before it, so `x` is not taken
/// from `ax` in `f(ax, x)`.
fn parameter_range(
    label: &str,
    parameters: &[ParameterInformation],
    index: usize,
) -> Option<Range<usize>> {
    let name_end = label.find('(').unwrap_or(0);
    let mut range = name_end..name_end;
    for parameter in parameters.get(..=index)? {
        range = match &parameter.label {
            ParameterLabel::Simple(text) => {
                let start = range.end + label[range.end..].find(text.as_str())?;
                start..start + text.len()
            }
            ParameterLabel::LabelOffsets([start, end]) => {
                utf16_byte(label, *start)?..utf16_byte(label, *end)?
            }
        };
    }
    Some(range)
}

fn utf16_byte(text: &str, units: u32) -> Option<usize> {
    let mut counted = 0;
    for (index, character) in text.char_indices() {
        if counted == units as usize {
            return Some(index);
        }
        counted += character.len_utf16();
    }
    (counted == units as usize).then_some(text.len())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn convert(help: Value) -> Signatures {
        signatures(serde_json::from_value(help).unwrap(), "TypeScript").unwrap()
    }

    fn emphasized(code: &Code) -> &str {
        &code.text[code.emphasis.clone().unwrap()]
    }

    #[test]
    fn parameters_are_found_by_offsets_or_text() {
        let help = json!({
            "signatures": [
                { "label": "f(ж: string, b: number): void",
                  "parameters": [{ "label": [2, 11] }, { "label": [13, 22] }] },
                { "label": "f(a: number, a2: number): void",
                  "parameters": [{ "label": "a: number" }, { "label": "a2: number" }],
                  "activeParameter": 0 },
                { "label": "f(ax, x)", "parameters": [{ "label": "ax" }, { "label": "x" }] },
            ],
            "activeSignature": 7,
            "activeParameter": 1,
        });
        let converted = convert(help);
        assert_eq!(converted.active, 2);
        assert_eq!(emphasized(&converted.list[0]), "b: number");
        assert_eq!(emphasized(&converted.list[1]), "a: number");
        assert_eq!(converted.list[1].mode, Some("TypeScript"));
        let x = converted.list[2].emphasis.clone().unwrap();
        assert_eq!(x, 6..7, "the second `x`, not the one in `ax`");
        let none = json!({ "signatures": [] });
        assert_eq!(signatures(serde_json::from_value(none).unwrap(), "C"), None);
    }

    #[test]
    fn typed_characters_ask_only_when_the_server_wants_them() {
        let support = SignatureSupport::from_capabilities(&json!({
            "signatureHelpProvider": { "triggerCharacters": ["(", ","], "retriggerCharacters": [")"] }
        }))
        .unwrap();
        let kind = |trigger| support.context(trigger).map(|context| context.trigger_kind);
        assert_eq!(kind(SignatureTrigger::Typed('a')), None);
        assert_eq!(kind(SignatureTrigger::Typed(')')), None);
        assert_eq!(
            kind(SignatureTrigger::Typed('(')),
            Some(SignatureHelpTriggerKind::TRIGGER_CHARACTER)
        );
        let update = support
            .context(SignatureTrigger::Update(Some(')')))
            .unwrap();
        assert_eq!(update.trigger_character.as_deref(), Some(")"));
        assert!(update.is_retrigger);
        assert_eq!(
            kind(SignatureTrigger::Update(Some('a'))),
            Some(SignatureHelpTriggerKind::CONTENT_CHANGE)
        );
        assert_eq!(SignatureSupport::from_capabilities(&json!({})), None);
    }
}
