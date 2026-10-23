; Regular expressions, painted over the string they are in: what is not a plain character
; stands out.

[
  (start_assertion) (end_assertion) (boundary_assertion) (non_boundary_assertion)
  (zero_or_more) (one_or_more) (optional) (count_quantifier) (lazy) (flags)
] @keyword
["|" "(" "(?:" "(?<" "(?P<" "(?" ")" "[" "]"] @keyword
[
  (any_character) (character_class_escape) (posix_character_class) (control_escape)
  (control_letter_escape) (identity_escape) (decimal_escape) (unicode_character_escape)
  (backreference_escape)
] @constant
(group_name) @label
