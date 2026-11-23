; Model extraction query for Python (tree-sitter-python 0.25). Starts from
; the grammar's own tags.scm; adds imports, decorators, fields, properties,
; constructors, async, annotations and parameters.
; Captures: see crates/model-treesitter/README.md.

; ---- definitions -----------------------------------------------------------

; Module-level assignments are constants; in a class body they are fields.
; Annotated (`x: int = 1`, `x: int`) are assignments too.
(module (expression_statement (assignment left: (identifier) @name) @definition.constant))
(class_definition body: (block
  (expression_statement (assignment left: (identifier) @name) @definition.field)))

(class_definition
  name: (identifier) @name) @definition.class
; Decorators sit outside `class_definition`: the decorated extent wins (same
; identifier keeps the outermost extent), so the decorators belong to it.
(decorated_definition
  definition: (class_definition name: (identifier) @name)) @definition.class

(function_definition
  name: (identifier) @name) @definition.function
(decorated_definition
  definition: (function_definition name: (identifier) @name)) @definition.function

; `__init__` is the constructor.
((function_definition name: (identifier) @name) @definition.constructor
  (#eq? @name "__init__"))
((decorated_definition
  definition: (function_definition name: (identifier) @name)) @definition.constructor
  (#eq? @name "__init__"))

; `@property` and its `@x.setter` / `@x.getter` / `@x.deleter`.
((decorated_definition
  (decorator (identifier) @_d)
  definition: (function_definition name: (identifier) @name)) @definition.property
  (#match? @_d "^(property|cached_property)$"))
((decorated_definition
  (decorator (attribute attribute: (identifier) @_d))
  definition: (function_definition name: (identifier) @name)) @definition.property
  (#match? @_d "^(setter|getter|deleter)$"))

; Parameters; `self` and `cls` are the implicit receiver, not entities.
((parameters (identifier) @name @definition.parameter)
  (#not-match? @name "^(self|cls)$"))
((parameters (default_parameter name: (identifier) @name) @definition.parameter)
  (#not-match? @name "^(self|cls)$"))
(parameters (typed_parameter (identifier) @name) @definition.parameter)
(parameters (typed_default_parameter name: (identifier) @name) @definition.parameter)
(parameters (list_splat_pattern (identifier) @name) @definition.parameter)
(parameters (dictionary_splat_pattern (identifier) @name) @definition.parameter)

; ---- relations ---------------------------------------------------------------

; `class A(B, mod.C)`: keyword arguments (`metaclass=`) are not supertypes.
(class_definition
  superclasses: (argument_list [
    (identifier) @name
    (attribute attribute: (identifier) @name)
  ])) @relation.inherits

; `import a.b`, `import a.b as c`, `from a.b import c`: the module read.
; Relative imports (`from . import x`) name no absolute module.
(import_statement name: (dotted_name) @name) @relation.imports
(import_statement name: (aliased_import name: (dotted_name) @name)) @relation.imports
(import_from_statement module_name: (dotted_name) @name) @relation.imports

; Every import statement, as written (aliases included), on its owner.
(import_statement) @prop.imports
(import_from_statement) @prop.imports

(call
  function: [
      (identifier) @name
      (attribute
        attribute: (identifier) @name)
  ]) @reference.call

; ---- decorators and modifiers -----------------------------------------------

; `@dec`, `@mod.dec`, `@dec(args)`, `@mod.dec(args)`: the dotted callee.
(decorator [
  (identifier) @prop.decorators
  (attribute) @prop.decorators
  (call function: [(identifier) (attribute)] @prop.decorators)
])

(function_definition "async" @prop.modifiers)

; ---- annotations --------------------------------------------------------------
; Types of parameters, returns, and class/module level variables. Builtins
; and `typing` generics name no project type.

(typed_parameter (identifier) @typed
  type: (type [
    (identifier) @name
    (attribute attribute: (identifier) @name)
    (generic_type (identifier) @name)
  ])
  (#not-match? @name "^(int|str|float|bool|bytes|complex|list|dict|set|frozenset|tuple|object|None|Any|Optional|Union|List|Dict|Set|Tuple|Callable|Iterable|Iterator|Sequence|Mapping|Type|Self)$")) @reference.declared_type
(typed_default_parameter name: (identifier) @typed
  type: (type [
    (identifier) @name
    (attribute attribute: (identifier) @name)
    (generic_type (identifier) @name)
  ])
  (#not-match? @name "^(int|str|float|bool|bytes|complex|list|dict|set|frozenset|tuple|object|None|Any|Optional|Union|List|Dict|Set|Tuple|Callable|Iterable|Iterator|Sequence|Mapping|Type|Self)$")) @reference.declared_type
(function_definition name: (identifier) @typed
  return_type: (type [
    (identifier) @name
    (attribute attribute: (identifier) @name)
    (generic_type (identifier) @name)
  ])
  (#not-match? @name "^(int|str|float|bool|bytes|complex|list|dict|set|frozenset|tuple|object|None|Any|Optional|Union|List|Dict|Set|Tuple|Callable|Iterable|Iterator|Sequence|Mapping|Type|Self)$")) @reference.declared_type
(module (expression_statement (assignment left: (identifier) @typed
  type: (type [
    (identifier) @name
    (attribute attribute: (identifier) @name)
    (generic_type (identifier) @name)
  ])
  (#not-match? @name "^(int|str|float|bool|bytes|complex|list|dict|set|frozenset|tuple|object|None|Any|Optional|Union|List|Dict|Set|Tuple|Callable|Iterable|Iterator|Sequence|Mapping|Type|Self)$")) @reference.declared_type))
(class_definition body: (block (expression_statement (assignment left: (identifier) @typed
  type: (type [
    (identifier) @name
    (attribute attribute: (identifier) @name)
    (generic_type (identifier) @name)
  ])
  (#not-match? @name "^(int|str|float|bool|bytes|complex|list|dict|set|frozenset|tuple|object|None|Any|Optional|Union|List|Dict|Set|Tuple|Callable|Iterable|Iterator|Sequence|Mapping|Type|Self)$")) @reference.declared_type)))
