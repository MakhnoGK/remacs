use remacs_syntax::Injections;

use crate::modes::{HTML, JAVASCRIPT, JSX, TSX, TYPESCRIPT};

/// Upstream's injects every script as JavaScript; here a script's `type` or `lang` picks
/// TypeScript or JSON too, and any other type (templates, `text/plain`) stays text. A
/// quoted `style` attribute is CSS and an `on…` one JavaScript, each ended by `;`, apart
/// from the page's styles and scripts.
const HTML_INJECTIONS: &str = r#"
((script_element
   (start_tag) @_tag
   (raw_text) @injection.content)
 (#not-match? @_tag "(?i)\\s(type|lang)\\s*=")
 (#set! injection.language "javascript")
 (#set! injection.combined))

((script_element
   (start_tag) @_tag
   (raw_text) @injection.content)
 (#match? @_tag "(?i)\\stype\\s*=\\s*[\"']?(module|(text|application)/(java|ecma)script)[\"'\\s>]")
 (#not-match? @_tag "(?i)\\slang\\s*=\\s*[\"']?ts[\"'\\s>]")
 (#set! injection.language "javascript")
 (#set! injection.combined))

((script_element
   (start_tag) @_tag
   (raw_text) @injection.content)
 (#match? @_tag "(?i)\\s(lang\\s*=\\s*[\"']?ts|type\\s*=\\s*[\"']?(text|application)/typescript)[\"'\\s>]")
 (#set! injection.language "typescript")
 (#set! injection.combined))

((script_element
   (start_tag) @_tag
   (raw_text) @injection.content)
 (#match? @_tag "(?i)\\stype\\s*=\\s*[\"']?(importmap|speculationrules|application/([a-z0-9.+-]+\\+)?json)[\"'\\s>]")
 (#set! injection.language "json")
 (#set! injection.combined))

((style_element
   (raw_text) @injection.content)
 (#set! injection.language "css")
 (#set! injection.combined))

((attribute
   (attribute_name) @_name
   (quoted_attribute_value) @injection.content)
 (#match? @_name "(?i)^style$")
 (#set! injection.language "css")
 (#set! injection.combined)
 (#set! injection.terminator ";"))

((attribute
   (attribute_name) @_name
   (quoted_attribute_value) @injection.content)
 (#match? @_name "(?i)^on[a-z]+$")
 (#set! injection.language "javascript")
 (#set! injection.combined)
 (#set! injection.terminator ";"))
"#;

/// Upstream's tagged templates, each its own code: upstream combines all of a file's, so
/// one `sql` query ran on into the next.
const TEMPLATE_INJECTIONS: &str = r#"
(call_expression
  function: [
    (identifier) @injection.language
    (member_expression
      property: (property_identifier) @injection.language)
  ]
  arguments: (template_string (string_fragment) @injection.content))
"#;

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
