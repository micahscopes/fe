; Comments
(line_comment) @comment

; Keywords
[
  "if"
  "else"
  "while"
  "for"
  "return"
  "fn"
  "struct"
  "enum"
  "pub"
] @keyword

; Function calls
(call_expression
  function: (identifier) @function)

; Function definitions
(function_definition
  name: (identifier) @function)

; Types
(type_identifier) @type

; Strings
(string_literal) @string

; Numbers
(number_literal) @number

; Variables
(identifier) @variable
