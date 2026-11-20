; Replace or extend this query with captures supported by the grammar.
(line_comment) @comment
(block_comment) @comment
(string_literal) @string
(integer_literal) @number
(type_identifier) @type
(function_item name: (identifier) @function)

[
  "fn"
  "let"
  "match"
  "struct"
  "enum"
  "impl"
  "trait"
  "use"
  "mod"
  "pub"
] @keyword
