; A tagged template is the language its tag names (sql`…`, css`…`, html`…`), each one code
; of its own, so that one query does not run on into the next.

(call_expression
  function: [
    (identifier) @injection.language
    (member_expression
      property: (property_identifier) @injection.language)
  ]
  arguments: (template_string (string_fragment) @injection.content))
