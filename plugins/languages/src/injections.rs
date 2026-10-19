use remacs_syntax::Injections;

use crate::modes::HTML;

/// Upstream's injects every script as JavaScript; here a script's `type` or `lang` picks
/// TypeScript or JSON too, and any other type (templates, `text/plain`) stays text.
const HTML_INJECTIONS: &str = r#"
((script_element
   (start_tag) @_tag
   (raw_text) @injection.content)
 (#not-match? @_tag "(?i)\\s(type|lang)\\s*=")
 (#set! injection.language "javascript"))

((script_element
   (start_tag) @_tag
   (raw_text) @injection.content)
 (#match? @_tag "(?i)\\stype\\s*=\\s*[\"']?(module|(text|application)/(java|ecma)script)[\"'\\s>]")
 (#not-match? @_tag "(?i)\\slang\\s*=\\s*[\"']?ts[\"'\\s>]")
 (#set! injection.language "javascript"))

((script_element
   (start_tag) @_tag
   (raw_text) @injection.content)
 (#match? @_tag "(?i)\\s(lang\\s*=\\s*[\"']?ts|type\\s*=\\s*[\"']?(text|application)/typescript)[\"'\\s>]")
 (#set! injection.language "typescript"))

((script_element
   (start_tag) @_tag
   (raw_text) @injection.content)
 (#match? @_tag "(?i)\\stype\\s*=\\s*[\"']?(importmap|speculationrules|application/([a-z0-9.+-]+\\+)?json)[\"'\\s>]")
 (#set! injection.language "json"))

((style_element
   (raw_text) @injection.content)
 (#set! injection.language "css"))
"#;

pub const INJECTIONS: &[Injections] = &[Injections {
    host: HTML,
    query: HTML_INJECTIONS,
}];
