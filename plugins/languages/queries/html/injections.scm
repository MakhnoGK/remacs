; A script is JavaScript, or by its `type` or `lang` TypeScript or JSON; any other type
; (templates, `text/plain`) stays text. A quoted `style` attribute is CSS and an `on…` one
; JavaScript, each ended by `;`, apart from the page's styles and scripts.

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
