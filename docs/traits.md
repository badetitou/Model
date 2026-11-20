# Traits across languages

Status: epic [#1](https://github.com/badetitou/Model/issues/1), step #13. The
traits of `model-traits` are generated from FamixJava (#6). This page records
what happens when TypeScript, Python and Pharo compose their kinds from them:
the trait concept is a mask on a kind, so a language only has to say which
traits its kinds carry.

## How a language defines its kinds

A kind is a row of the `Metamodel`: name, archetype (`EntityKind`), trait mask,
optional FM3 class name.

- **tree-sitter languages** (Java, TypeScript, Python): rows live in
  `crates/model-treesitter/kinds.json`; a query file selects them with
  `@definition.<kind name>` (`@definition.ts.interface`). No per-language Rust.
  A grammar row may name a `file_kind` for its file entities (`python.module`).
- **Pharo**: `model-lang-pharo` has no grammar; its four rows are a `const`
  table in `lib.rs`.

Trait names refer to `model-traits`; an unknown name is ignored. The FM3 class
names of TypeScript and Smalltalk (`Famix-TypeScript-Entities.*`,
`Famix-Smalltalk-Entities.*`) are written from the Famix repositories' naming
and have **not** been checked against an FM3 export (none was available):
verify them when the Moose crates exercise them.

## TypeScript

| Kind | Archetype | Traits | Famix-TypeScript |
|---|---|---|---|
| `ts.interface` | Interface | Named, WithMembers, WithInheritances, Typed | `Interface` |
| `ts.type_alias` | TypeAlias | Named, Typed | `TypeAlias` |
| `ts.namespace` | Namespace | Named, WithMembers | `Namespace` |
| `ts.abstract_class` | Class | Named, WithMembers, WithInheritances, Modifiers, Annotations | `Class` |
| `ts.arrow_function` | Function | Named, Invocation, Access, Typed, Modifiers | `ArrowFunction` |
| `ts.module` (file) | File | Named, WithMembers | `Module` |

Other TypeScript entities (class, enum, method, field...) keep their archetype
kind. Not done: parameters, locals, type parameters, decorators as
`AnnotationInstance`, `declared_type`/`accesses` captures (the Java ones need a
TypeScript query each), imports.

## Python

| Kind | Archetype | Traits |
|---|---|---|
| `python.module` (file) | File | Named, WithMembers |
| `parameter` (shared with Java) | Variable | none |

`queries/python.scm` starts from the grammar's `tags.scm` and adds
superclasses (`Inherits`) and parameters (`self`/`cls` excluded). There is no
official Famix Python metamodel, so no FM3 name. Not done: async functions
(a function directly in a class becomes a method only when it has no fine
kind), decorators, imports, `declared_type` from annotations.

## Pharo against Famix-Smalltalk

| Kind | Archetype | Traits | Famix-Smalltalk |
|---|---|---|---|
| `pharo.class` | Class | Named, WithMembers, WithInheritances, Modifiers | `Class` |
| `pharo.trait` | Interface | Named, WithMembers, Modifiers | `Trait` |
| `pharo.method` | Method | Named, Invocation, Access, Modifiers | `Method` |
| `pharo.extension_method` | Method | same | none |

Pharo is the second composability test: the trait set was built from Java and
Pharo needs none of the Java-only ones (Typed, Exceptions, Annotations: Pharo
is untyped, and its pragmas/exceptions are not extracted). Composition by
omission works: Pharo kinds are plain subsets of the Java traits, and
`of_trait(Invocation|Access)` returns Pharo methods next to Java ones.
**The trait set did not need to change.**

Used as is:

- `Modifiers` (`TCanBeClassSide` among its FM3 traits) covers class-side: a
  class-side method is a `pharo.method` with prop `pharo.classSide`, as in
  Famix-Smalltalk where class-side is a property, not a kind.
- `Invocation` carries `Calls` (hand-resolved sends, `Provenance::Frontend`),
  `Access`/`References` carries class references, `WithInheritances` carries
  `Inherits`.

No trait yet, and why:

| Pharo concept | Today | Missing |
|---|---|---|
| Trait usage (`uses: TFoo`) | not extracted | a `TraitUsage` trait and a `uses` relation kind (`TTraitUsage`/`TTrait` exist in Famix, not in the Java-derived set) |
| Extensions | `pharo.extension_method` kind, same traits | a Famix class (none) and the extended class/package link (`ExtensionOf`); the extending package is only the `pharo.package` prop |
| Stubs (referenced, undefined classes) | external `Class` entity (`set_external`), default kind | a `Stub` trait/kind; Famix-Smalltalk marks them with `isStub` |
| Metaclass (class side as an entity) | prop `pharo.classSide` on methods | a `Metaclass` kind, if consumers want `Foo class` as a type |
| Packages and tags | prop `pharo.package`, no entity | a `Package`/`Tag` entity and `Namespace`-like containment |
| Instance/class variables, pools | not extracted (only `classVars` names to filter references) | `Attribute` entities (`TWithAttributes` is in `WithMembers`, so the trait is ready) |
| Protocols (method categories) | not extracted | none in Famix either (a plain property) |

If one of these is added, the traits are: `TraitUsage` as a new entry of
`traits.json` (+ a `RelationKind`), the others are kinds composed from the
existing traits.
