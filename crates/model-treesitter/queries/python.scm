; Model extraction query for Python (tree-sitter-python 0.25). Starts from
; the grammar's own tags.scm, adds superclasses and parameters.
; Captures: see crates/model-treesitter/README.md.

(module (expression_statement (assignment left: (identifier) @name) @definition.constant))

(class_definition
  name: (identifier) @name) @definition.class

(function_definition
  name: (identifier) @name) @definition.function

; `class A(B, mod.C)`: keyword arguments (`metaclass=`) are not supertypes.
(class_definition
  superclasses: (argument_list [
    (identifier) @name
    (attribute attribute: (identifier) @name)
  ])) @relation.inherits

; Parameters; `self` and `cls` are the implicit receiver, not entities.
((parameters (identifier) @name @definition.parameter)
  (#not-match? @name "^(self|cls)$"))
((parameters (default_parameter name: (identifier) @name) @definition.parameter)
  (#not-match? @name "^(self|cls)$"))
(parameters (typed_parameter (identifier) @name) @definition.parameter)
(parameters (typed_default_parameter name: (identifier) @name) @definition.parameter)

(call
  function: [
      (identifier) @name
      (attribute
        attribute: (identifier) @name)
  ]) @reference.call
