; Model extraction query for Java (tree-sitter-java 0.23).
; Captures: see crates/model-treesitter/README.md.

(package_declaration [(identifier) (scoped_identifier)] @name) @namespace

; Single-type imports only: wildcard (`.*`, not last child) and static
; imports name no type.
((import_declaration [(identifier) (scoped_identifier)] @name .) @relation.imports
  (#not-match? @relation.imports "^import\\s+static"))

(class_declaration name: (identifier) @name) @definition.class
(record_declaration name: (identifier) @name) @definition.record
(interface_declaration name: (identifier) @name) @definition.interface
(annotation_type_declaration name: (identifier) @name) @definition.annotation_type
(enum_declaration name: (identifier) @name) @definition.enum
(method_declaration name: (identifier) @name) @definition.method
(constructor_declaration name: (identifier) @name) @definition.constructor
(field_declaration declarator: (variable_declarator name: (identifier) @name)) @definition.field
(constant_declaration declarator: (variable_declarator name: (identifier) @name)) @definition.constant
(enum_constant name: (identifier) @name) @definition.enum_constant

; Entities below a member: parameters, locals, type parameters,
; annotation instances and comments (FamixJava's Parameter, LocalVariable,
; ParameterType, AnnotationInstance, Comment).
(formal_parameter name: (identifier) @name) @definition.parameter
(spread_parameter (variable_declarator name: (identifier) @name)) @definition.parameter
(catch_formal_parameter name: (identifier) @name) @definition.parameter
(inferred_parameters (identifier) @name @definition.parameter)
(lambda_expression parameters: (identifier) @name @definition.parameter)

(local_variable_declaration
  declarator: (variable_declarator name: (identifier) @name) @definition.local_variable)
(enhanced_for_statement name: (identifier) @name @definition.local_variable)
(resource name: (identifier) @name) @definition.local_variable

(type_parameter (type_identifier) @name) @definition.type_parameter

(marker_annotation name: _ @name) @definition.annotation
(annotation name: _ @name) @definition.annotation

[(line_comment) (block_comment)] @name @definition.comment

(superclass [
  (type_identifier) @name
  (scoped_type_identifier) @name
  (generic_type [(type_identifier) (scoped_type_identifier)] @name)
]) @relation.inherits

(super_interfaces (type_list [
  (type_identifier) @name
  (scoped_type_identifier) @name
  (generic_type [(type_identifier) (scoped_type_identifier)] @name)
])) @relation.implements

(extends_interfaces (type_list [
  (type_identifier) @name
  (scoped_type_identifier) @name
  (generic_type [(type_identifier) (scoped_type_identifier)] @name)
])) @relation.implements

(method_invocation name: (identifier) @name) @reference.call

(object_creation_expression type: [
  (type_identifier) @name
  (scoped_type_identifier) @name
  (generic_type [(type_identifier) (scoped_type_identifier)] @name)
]) @reference.type

(marker_annotation name: _ @prop.annotations)
(annotation name: _ @prop.annotations)

; Relations of FamixJava's typed entities, exceptions and accesses. `@typed`
; names the entity the type belongs to (a parameter or local is not the
; owner of its position); accesses resolve by name to a parameter, local or
; field of an enclosing type, and are dropped when unknown.
(method_declaration type: [
  (type_identifier) @name
  (scoped_type_identifier) @name
  (generic_type [(type_identifier) (scoped_type_identifier)] @name)
  (array_type element: (type_identifier) @name)
] name: (identifier) @typed
  (#not-eq? @name "var")) @reference.declared_type

(formal_parameter type: [
  (type_identifier) @name
  (scoped_type_identifier) @name
  (generic_type [(type_identifier) (scoped_type_identifier)] @name)
  (array_type element: (type_identifier) @name)
] name: (identifier) @typed
  (#not-eq? @name "var")) @reference.declared_type

(field_declaration type: [
  (type_identifier) @name
  (scoped_type_identifier) @name
  (generic_type [(type_identifier) (scoped_type_identifier)] @name)
  (array_type element: (type_identifier) @name)
] declarator: (variable_declarator name: (identifier) @typed)
  (#not-eq? @name "var")) @reference.declared_type

(constant_declaration type: [
  (type_identifier) @name
  (scoped_type_identifier) @name
  (generic_type [(type_identifier) (scoped_type_identifier)] @name)
  (array_type element: (type_identifier) @name)
] declarator: (variable_declarator name: (identifier) @typed)
  (#not-eq? @name "var")) @reference.declared_type

(local_variable_declaration type: [
  (type_identifier) @name
  (scoped_type_identifier) @name
  (generic_type [(type_identifier) (scoped_type_identifier)] @name)
  (array_type element: (type_identifier) @name)
] declarator: (variable_declarator name: (identifier) @typed)
  (#not-eq? @name "var")) @reference.declared_type

(throws [
  (type_identifier) @name
  (scoped_type_identifier) @name
]) @reference.throws

(assignment_expression left: [(identifier) @name (field_access object: (this) field: (identifier) @name)] operator: "=") @access.write
(assignment_expression left: [(identifier) @name (field_access object: (this) field: (identifier) @name)] operator: ["+=" "-=" "*=" "/=" "%=" "&=" "|=" "^=" "<<=" ">>=" ">>>="]) @access.read_write
(update_expression [(identifier) @name (field_access object: (this) field: (identifier) @name)]) @access.read_write

(argument_list [(identifier) @name (field_access object: (this) field: (identifier) @name)]) @access.read
(binary_expression left: [(identifier) @name (field_access object: (this) field: (identifier) @name)]) @access.read
(binary_expression right: [(identifier) @name (field_access object: (this) field: (identifier) @name)]) @access.read
(assignment_expression right: [(identifier) @name (field_access object: (this) field: (identifier) @name)]) @access.read
(variable_declarator value: [(identifier) @name (field_access object: (this) field: (identifier) @name)]) @access.read
(return_statement [(identifier) @name (field_access object: (this) field: (identifier) @name)]) @access.read
(parenthesized_expression [(identifier) @name (field_access object: (this) field: (identifier) @name)]) @access.read
(unary_expression operand: [(identifier) @name (field_access object: (this) field: (identifier) @name)]) @access.read
(array_access array: [(identifier) @name (field_access object: (this) field: (identifier) @name)]) @access.read
(array_access index: [(identifier) @name (field_access object: (this) field: (identifier) @name)]) @access.read
(ternary_expression condition: [(identifier) @name (field_access object: (this) field: (identifier) @name)]) @access.read
(ternary_expression consequence: [(identifier) @name (field_access object: (this) field: (identifier) @name)]) @access.read
(ternary_expression alternative: [(identifier) @name (field_access object: (this) field: (identifier) @name)]) @access.read
(cast_expression value: [(identifier) @name (field_access object: (this) field: (identifier) @name)]) @access.read
(enhanced_for_statement value: [(identifier) @name (field_access object: (this) field: (identifier) @name)]) @access.read

(method_invocation object: [(identifier) @name (field_access object: (this) field: (identifier) @name)]) @access.receiver
(field_access object: (identifier) @name) @access.receiver

; Cyclomatic complexity: one decision point per branch. `@complexity.branch`
; needs no `@name`; model-treesitter counts unique captured nodes and stores
; the count on the enclosing method.
[
  (if_statement)
  (for_statement)
  (enhanced_for_statement)
  (while_statement)
  (do_statement)
  (switch_expression)
  (catch_clause)
  (ternary_expression)
] @complexity.branch

(binary_expression operator: ["&&" "||"]) @complexity.branch
