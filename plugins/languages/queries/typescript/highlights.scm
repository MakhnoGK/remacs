; TypeScript's types and declarations, after the ECMAScript patterns.

(type_identifier) @type
(predefined_type) @type.builtin
(this_type) @variable.builtin
(enum_declaration name: (identifier) @type)
(enum_body name: (property_identifier) @constant)
(enum_assignment name: (property_identifier) @constant)
(function_signature name: (identifier) @function)
(method_signature name: (property_identifier) @function)
(abstract_method_signature name: (property_identifier) @function)

[
  "abstract" "accessor" "asserts" "declare" "enum" "global" "implements" "infer"
  "interface" "is" "keyof" "module" "namespace" "override" "readonly" "satisfies" "type"
] @keyword
(accessibility_modifier) @keyword
(override_modifier) @keyword
