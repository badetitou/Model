# model-treesitter

Extracts a model from sources with tree-sitter. Extraction is driven by a
query file per language, not by per-language code: supporting a language is
a grammar crate (behind a Cargo feature), a query, and a row in
`src/grammars.rs`.

| Language | Grammar | Query | Namespaces |
|---|---|---|---|
| Java | tree-sitter-java 0.23 | `queries/java.scm` | declared (`package`) |
| TypeScript, TSX | tree-sitter-typescript 0.23 | `queries/typescript.scm` | file path |
| Python | tree-sitter-python 0.25 | the grammar's own `tags.scm`, unchanged | file path |

## Captures

The [tree-sitter tags](https://tree-sitter.github.io/tree-sitter/4-code-navigation.html)
convention is understood as is, so a grammar's `tags.scm` is a working
baseline. Model adds a few captures.

| Capture | Meaning |
|---|---|
| `@definition.<kind>` + `@name` | an entity; `<kind>`: `class`, `interface`, `trait`/`mixin`, `enum`, `struct`/`union`, `type`/`type_alias`, `method`, `constructor`, `function`/`macro`, `field`, `property`, `constant`, `variable`, `namespace`/`module` |
| `@namespace` + `@name` | the file's declared namespace (`package a.b;`) |
| `@relation.inherits` / `@relation.implements` + `@name` | supertype of the enclosing definition (tags: `@reference.implementation`) |
| `@relation.imports` + `@name` | imported qualified name, from the file |
| `@reference.call` + `@name` | call by name |
| `@reference.type` + `@name` | use of a type by name, e.g. `new X()` (tags: `@reference.class`) |
| `@prop.<key>` | the captured text is added to the `<key>` list prop of the innermost enclosing definition(s) |

Other captures (`@doc`, `@_x`...) are ignored; text predicates (`#eq?`,
`#match?`...) apply.

## What the extractor does with them

- **Nesting** comes from extents: a definition's parent is the innermost
  definition strictly containing it. Queries stay flat. A `function`
  directly inside a type becomes a `method`.
- **One identifier, one entity**: when several patterns match the same
  identifier, the outermost extent wins (TS `@Dec export class A` starts at
  the decorator).
- **Keys** are SCIP-like: `model java . . com/acme/A#run().`. Constructors
  use the indexer's name (`<init>`, `<constructor>`). Overloads are numbered
  in declaration order (`m().`, `m(+1).`), as SCIP indexers do. Files are
  keyed by path (`model typescript . . src/app/`a.ts`/`).
- **Namespaces**: `Declared` languages get namespace entities from
  `@namespace` and files are roots. `Path` languages get directory
  namespaces, and top-level definitions live under their `File`.
- **Relations** are resolved by name, best effort, `Provenance::TreeSitter`,
  every site kept:
  - types: qualified name, then import, then same namespace or file, then
    the only project type of that name, else an external entity;
  - calls: a callable of that name in the enclosing type, else the only one
    in the project, else unresolved (counted).

  With a SCIP index, extract with `relations: false` and let `model-link`
  resolve them exactly. Definitions, extents and props still come from here.

## Measured, with `model-link` (`examples/build_model.rs`)

| Fixture | Tree-sitter | SCIP definitions mapped (+ lifted) | Total |
|---|---|---|---|
| jpetstore (46 Java files) | 60 ms | 94.5% (100%) | 70 ms |
| angular-realworld (50 TS files) | 45 ms | 96.7% (98.9%) | 50 ms |
| vscode (15.7k TS files, 1 GB index) | 3.1 s | 88.8% (94.6%) | 6.5 s, 197 MiB model |

`tests/fixtures.rs` checks these against Structure's fixtures (set
`MODEL_FIXTURES` to an absolute path).

## Known limits

- Name resolution ignores receivers and inheritance (`a.save()` resolves to
  "the" `save` only when it is unique).
- TypeScript imports are module paths: not captured yet.
- Java nested types in imports (`a.B.C`) are read as namespace `B`.
- Method decorators that the grammar puts outside the method node attach to
  the enclosing class.
