; Extends C's.
[
  (declaration_list)
  (template_argument_list)
  (template_parameter_list)
  (condition_clause)
  (field_initializer_list)
] @indent.begin

; `public:` stands at its class's level.
(access_specifier) @indent.branch

(template_argument_list ">" @indent.end)

(template_parameter_list ">" @indent.end)
