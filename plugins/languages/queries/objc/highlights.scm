; Objective-C, after C's patterns.

(message_expression method: (identifier) @function.call)
(method_declaration (identifier) @function)
(method_definition (identifier) @function)
(keyword_declarator (identifier) @function)
(class_interface (identifier) @type)
(class_implementation (identifier) @type)
(protocol_declaration (identifier) @type)
(selector_expression) @constant
((identifier) @variable.builtin (#any-of? @variable.builtin "self" "super"))
((identifier) @boolean (#any-of? @boolean "YES" "NO"))
((identifier) @constant.builtin (#any-of? @constant.builtin "nil" "Nil"))
["BOOL" "Class" "IMP" "SEL" "id"] @type.builtin

["#import" "@import"] @preproc
[
  "@autoreleasepool" "@catch" "@dynamic" "@end" "@finally" "@implementation"
  "@interface" "@optional" "@package" "@private" "@property" "@protected" "@protocol"
  "@public" "@required" "@selector" "@synchronized" "@synthesize" "@throw" "@try"
  "@encode" "@available" "bycopy" "byref" "in" "inout" "oneway" "out"
] @keyword
