; Model extraction query for Java (tree-sitter-java 0.23).
; Captures: see crates/model-treesitter/README.md.

(package_declaration [(identifier) (scoped_identifier)] @name) @namespace

; Single-type imports only: wildcard (`.*`, not last child) and static
; imports name no type.
((import_declaration [(identifier) (scoped_identifier)] @name .) @relation.imports
  (#not-match? @relation.imports "^import\\s+static"))

(class_declaration name: (identifier) @name) @definition.class
(record_declaration name: (identifier) @name) @definition.class
(interface_declaration name: (identifier) @name) @definition.interface
(annotation_type_declaration name: (identifier) @name) @definition.interface
(enum_declaration name: (identifier) @name) @definition.enum
(method_declaration name: (identifier) @name) @definition.method
(constructor_declaration name: (identifier) @name) @definition.constructor
(field_declaration declarator: (variable_declarator name: (identifier) @name)) @definition.field
(constant_declaration declarator: (variable_declarator name: (identifier) @name)) @definition.constant
(enum_constant name: (identifier) @name) @definition.constant

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
