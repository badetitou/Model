; Model extraction query for TypeScript and TSX (tree-sitter-typescript 0.23).
; Captures: see crates/model-treesitter/README.md.

(class_declaration name: (type_identifier) @name) @definition.class
(abstract_class_declaration name: (type_identifier) @name) @definition.ts.abstract_class
; Decorators of an exported class belong to the export statement: the class
; extent starts there (the same identifier keeps the outermost extent).
(export_statement (decorator)
  declaration: (class_declaration name: (type_identifier) @name)) @definition.class
(export_statement (decorator)
  declaration: (abstract_class_declaration name: (type_identifier) @name)) @definition.ts.abstract_class
(interface_declaration name: (type_identifier) @name) @definition.ts.interface
(enum_declaration name: (identifier) @name) @definition.enum
(type_alias_declaration name: (type_identifier) @name) @definition.ts.type_alias
(function_declaration name: (identifier) @name) @definition.function
(generator_function_declaration name: (identifier) @name) @definition.function
(internal_module name: [(identifier) (nested_identifier)] @name) @definition.ts.namespace

((method_definition name: (property_identifier) @name) @definition.constructor
  (#eq? @name "constructor"))
((method_definition name: [(property_identifier) (private_property_identifier)] @name) @definition.method
  (#not-eq? @name "constructor"))
(method_signature name: (property_identifier) @name) @definition.method
(abstract_method_signature name: (property_identifier) @name) @definition.method
(public_field_definition name: [(property_identifier) (private_property_identifier)] @name) @definition.field
(property_signature name: (property_identifier) @name) @definition.property

; Top-level `const f = () => ...`, exported or not.
(program (lexical_declaration
  (variable_declarator name: (identifier) @name value: [(arrow_function) (function_expression)])) @definition.ts.arrow_function)
(program (export_statement (lexical_declaration
  (variable_declarator name: (identifier) @name value: [(arrow_function) (function_expression)])) @definition.ts.arrow_function))

(extends_clause value: [
  (identifier) @name
  (member_expression property: (property_identifier) @name)
]) @relation.inherits

(implements_clause [
  (type_identifier) @name
  (generic_type name: (type_identifier) @name)
]) @relation.implements

(extends_type_clause type: [
  (type_identifier) @name
  (generic_type name: (type_identifier) @name)
]) @relation.implements

(call_expression function: [
  (identifier) @name
  (member_expression property: (property_identifier) @name)
]) @reference.call

(new_expression constructor: (identifier) @name) @reference.type

(decorator [
  (identifier) @prop.decorators
  (call_expression function: (identifier) @prop.decorators)
])

; Cyclomatic complexity: one decision point per branch. `@complexity.branch`
; needs no `@name`; model-treesitter counts unique captured nodes and stores
; the count on the enclosing member.
[
  (if_statement)
  (for_statement)
  (for_in_statement)
  (while_statement)
  (do_statement)
  (switch_statement)
  (catch_clause)
  (ternary_expression)
] @complexity.branch

(binary_expression operator: ["&&" "||" "??"]) @complexity.branch
