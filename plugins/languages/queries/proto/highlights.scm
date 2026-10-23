; Protocol Buffers.

[(message_name) (enum_name) (service_name)] @type
(message_or_enum_type) @type
(rpc_name) @function
(field (identifier) @property)
(map_field (identifier) @property)
(oneof_field (identifier) @property)
(enum_field (identifier) @constant)
(option (identifier) @attribute)
(field_option (identifier) @attribute)
(package (full_ident) @type)

(string) @string
(escape_sequence) @string.escape
[(int_lit) (float_lit) (field_number)] @number
[(true) (false)] @boolean
(comment) @comment
((comment) @comment.doc (#match? @comment.doc "^(///|/\\*\\*)"))

[
  "bool" "bytes" "double" "fixed32" "fixed64" "float" "int32" "int64" "sfixed32" "sfixed64"
  "sint32" "sint64" "string" "uint32" "uint64"
] @type.builtin
(key_type) @type.builtin

[
  "edition" "enum" "export" "extend" "extensions" "group" "import" "local" "map" "max"
  "message" "oneof" "option" "optional" "package" "public" "repeated" "required"
  "reserved" "returns" "rpc" "service" "stream" "syntax" "to" "weak"
] @keyword
