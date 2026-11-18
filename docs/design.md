# Model — design

Status: draft (skeleton: `model-core`, `model-detect`).

## 1. Goal

Build a **language-neutral model of a codebase** from two technologies:

- **tree-sitter** — fast, error-tolerant syntax for almost every language:
  entities, their extents, containment, syntactic facts (annotations,
  modifiers, written supertypes, call sites);
- **SCIP** — compiler-grade symbol resolution produced by per-language
  indexers: stable symbol keys, definitions, references, implementations.

The language is **detected automatically**. The model is then the input of
**analyses** (model-driven engineering: one metamodel, many
transformations/queries), and can be exposed by **plugins** — the first one
being a GraphQL server.

Model is extracted from [Structure](https://github.com/badetitou/Structure),
which becomes its first client (classification, dependency matrix, git impact).

Non-goals: being a compiler, a full AST store, or a FAMIX/Moose replacement.
The metamodel is small on purpose; an exporter to FAMIX/MSE is a possible
plugin.

## 2. Repository and crates

Standalone repository `badetitou/Model`, a Cargo workspace (edition 2024).
Clients depend on it via Cargo git dependencies:

```toml
model-core = { git = "https://github.com/badetitou/Model", tag = "v0.1.0" }
```

```
model-core         metamodel + builder + navigation API      (serde only)
  ^
  ├── model-detect      language detection, project roots        ✅ skeleton
  ├── model-treesitter  generic extractor driven by queries      ⏳
  ├── model-scip        SCIP loading, indexer runner, cache      ⏳
  ├── model-link        tree-sitter ⨝ SCIP fusion                ⏳
  ├── model-lang-*      optional per-language enrichers          ⏳
  └── model-analysis    Analysis trait + generic analyses        ⏳
        ^
  model (facade)  `Model::build(path, &Options)` : detect → index → extract → link
        ^
  model-cli        `model detect|build|query <path>`             ⏳
  model-graphql    GraphQL plugin (part 2)                      ⏳
```

Dependencies only point up to `model-core`. Grammar crates live behind Cargo
features (`lang-java`, `lang-python`, ...; `default = ["all-languages"]`) so a
client only compiles the grammars it needs (Structure's WASM build in
particular).

## 3. Metamodel (`model-core`)

```
Model
 ├── entities: [Entity]
 │     id        EntityId (index, local to the model)
 │     key       stable unique key — SCIP symbol, else frontend qualified name
 │     kind      Namespace | File | Class | Interface | Trait | Enum | Struct
 │               | TypeAlias | Method | Constructor | Function
 │               | Field | Property | Constant | Variable
 │     name      simple name
 │     parent    containment (Namespace ⊃ Type ⊃ Method ...)
 │     language  open id: "java", "python", "pharo"...
 │     external  referenced, not defined in the sources (libraries)
 │     source    { file, range, name_range }
 │     props     open map: "java.annotations", "visibility", "http.endpoints"...
 └── relations: [Relation]
       kind        Calls | References | Inherits | Implements | Imports | Other(String)
       from, to    EntityId
       provenance  TreeSitter | Scip | Frontend | Derived
       source      occurrence site (optional)
```

Design choices:

- **Closed entity kinds, open everything else.** Analyses can match
  exhaustively on `EntityKind`; language-specific facts go into `props`,
  plugin relations into `RelationKind::Other` (serialised as a plain string:
  `"calls"`, `"http"`).
- **Containment is `parent`, not a relation**: it is a tree, and almost every
  analysis needs it.
- **`key` is the identity across runs**; `EntityId` is a dense index for
  speed. The builder is get-or-create by key, which is how tree-sitter and
  SCIP facts about the same symbol land on one entity.
- **`provenance` on relations** lets an analysis decide whether to trust a
  syntactic guess (`TreeSitter`) or only resolved facts (`Scip`).
- **Occurrences are kept** (one relation per site, with `source`);
  aggregation (dependency weight, matrix cells) is an analysis concern.
- Serialised as JSON `{ entities, relations }`; indexes (by key, children,
  incoming, outgoing) are rebuilt on load.

Mapping from Structure's IR: `Symbol` → `Entity` (`SymbolKind` maps 1:1;
`annotations`, `supertypes`, `http`, `http_calls`, `rpc_calls` → `props`),
`Symbol.package` → parent `Namespace`, `Edge` → `Relation`
(`Extends` → `Inherits`, `Uses` → `References`, `Http`/`Rpc` → `Other`),
`ext:<Name>` targets → `external` entities.

## 4. Pipeline

```
detect ──► for each detected language / project root:
             index (SCIP, optional, cached) ─┐
             extract (tree-sitter)  ─────────┴─► link ──► ModelBuilder ──► Model
```

1. **Detect** (`model-detect`, done). One scan honouring `.gitignore`
   (`ignore` crate) counts source files per extension and records directories
   holding build markers (`pom.xml`, `package.json`, `go.mod`, `*.csproj`,
   Tonel `.properties`...). Output: every language present, with its file
   count and project roots, so monorepos (Java backend + TS frontend) are
   handled without configuration. The language table is data
   (`LanguageSpec`): adding a language is adding a row.
2. **Index**. For each language with a SCIP indexer and a project root, run
   it (`scip-java`, `scip-typescript`, `scip-python`, `scip-go`,
   `rust-analyzer scip`, `scip-clang`, `scip-dotnet`, `scip-ruby`...), with
   timeout, progress and the per-document cache already in Structure
   (`pipeline/indexer.rs`, `scip_cache.rs`). Missing indexer = warning, not an
   error.
3. **Extract**. Generic tree-sitter extractor (§5).
4. **Link**. Generalisation of Structure's `link.rs`: SCIP definition
   occurrences are matched to tree-sitter entities by `(file, name_range)`,
   the entity takes the SCIP symbol as `key`, and reference occurrences become
   `Calls` / `References` relations from the innermost enclosing entity;
   SCIP `relationships` (`is_implementation`) become `Implements`/`Inherits`.
5. **Build**. Cross-language relations (HTTP, RPC) are *not* built here: they
   are analyses producing `Derived` relations (§7).

Degraded modes are first class:

| Available | Result |
|---|---|
| grammar + indexer | full model (`Scip` relations) |
| grammar only | entities + syntactic relations (`TreeSitter`, names resolved best-effort) |
| indexer only | entities from SCIP `SymbolInformation` (`kind`, `enclosing_symbol`, `enclosing_range`) + resolved relations, no syntactic props |

## 5. Generic tree-sitter extraction

Writing one extractor per language does not scale to "any language". Instead
the extractor is **driven by queries**, following the
[tree-sitter tags convention](https://tree-sitter.github.io/tree-sitter/4-code-navigation.html)
that most grammars already ship as `queries/tags.scm`:

```scheme
(class_declaration name: (identifier) @name) @definition.class
(method_declaration name: (identifier) @name) @definition.method
(method_invocation name: (identifier) @name) @reference.call
```

- Captures map to entity kinds (`@definition.class` → `Class`,
  `@definition.method` → `Method`, ...); `@name` gives `name_range`.
- **Containment is inferred from range nesting** (innermost enclosing
  definition), so queries stay flat.
- Model ships its own `queries/<lang>/model.scm` when it needs more than
  `tags.scm` gives, with extra captures: `@definition.field`,
  `@definition.namespace`, `@relation.inherits`, `@relation.implements`,
  `@relation.imports`, `@prop.<name>` (annotation, modifier, visibility).
  Fallback: the grammar's `tags.scm`.
- **Enrichers** (`model-lang-*`) are the escape hatch for facts a query
  cannot express (Java/TS HTTP endpoints, GWT RPC, Pharo send resolution).
  They receive the tree and the builder and only add `props`/relations.

So a new language needs: a grammar crate, a query file (often just its
`tags.scm`), optionally a SCIP indexer row. No Rust code.

Initial matrix:

| Language | Grammar | Queries | SCIP indexer |
|---|---|---|---|
| Java | tree-sitter-java | model.scm (from Structure) | scip-java |
| TypeScript/JS | tree-sitter-typescript / -javascript | model.scm (from Structure) | scip-typescript |
| Pharo (Tonel) | tree-sitter-tonel-smalltalk (see below) | model.scm to write | none — `Frontend` enricher |
| Python, Go, Rust, C/C++, C#, Ruby, PHP | official grammars | `tags.scm` | scip-python, scip-go, rust-analyzer, scip-clang, scip-dotnet, scip-ruby, scip-php |

### Pharo / Tonel

[`tree-sitter-tonel-smalltalk`](https://github.com/mumez/tree-sitter-tonel-smalltalk)
(MIT) parses Tonel `.st` files: STON class/trait/extension definitions and
method bodies. It replaces Structure's hand-written Tonel reader for
extraction. Notes:

- Its Rust binding is the pre-0.23 style (`tree-sitter = "~0.26.8"`, not on
  crates.io), which conflicts with our `tree-sitter 0.27`. Options: open an
  upstream PR moving it to `tree-sitter-language` (`LanguageFn`, version
  independent), or vendor its generated `src/parser.c` in
  `model-lang-pharo` with a `build.rs`. Upstream PR preferred.
- No `tags.scm` upstream: write `model.scm` (class/trait/extension
  definitions, `#superclass`, `#traits`, `#category`, method definitions with
  class side, message sends as `@reference.call`).
- No SCIP indexer for Pharo: the send resolution of Structure's
  `lang-pharo/sends.rs` (self/super, class receivers, unique selectors)
  becomes the Pharo enricher, producing `Calls` with
  `Provenance::Frontend`.

## 6. Navigation API

`Model` exposes: `entities()`, `relations()`, `entity(id)`, `by_key(key)`,
`of_kind(kind)`, `roots()`, `parent(id)`, `ancestors(id)`, `children(id)`,
`outgoing(id)`, `incoming(id)`, `targets(id, kind)`. All lookups return
`Option`/iterators, never panic, even on hand-edited JSON.

Next additions, driven by the first analyses: `descendants`, filtered
iterators by language/file, a `ModelView` (sub-model restricted to a scope:
package, files changed in a diff).

## 7. Analyses (MDE)

```rust
pub trait Analysis {
    type Output;
    fn name(&self) -> &str;
    fn run(&self, model: &Model) -> anyhow::Result<Self::Output>;
}
```

- Read-only analyses return a value (metrics, dead code, cycles,
  layer violations, impact of a diff).
- **Transformations** return a new `Model` (or relations to add with
  `Provenance::Derived`): HTTP linking (TS `http.calls` ⨝ Java
  `http.endpoints`), GWT RPC linking, quotient by package/layer.

Generic analyses that belong in `model-analysis`: fan-in/fan-out, dependency
cycles (Tarjan), unused entities, quotient graph by an arbitrary
entity → group function. Structure keeps its domain-specific ones:
`layer × domain` classification, matrix, git impact.

## 8. Plugins — GraphQL (part 2)

`model-graphql` (async-graphql + axum) serves a built `Model` (in memory, or
loaded from `model.json`):

```graphql
type Query {
  languages: [String!]!
  entity(key: String!): Entity
  entities(kind: EntityKind, language: String, name: String,
           first: Int, after: String): EntityConnection!
}

type Entity {
  key: String!            kind: EntityKind!     name: String!
  language: String        external: Boolean!    source: SourceAnchor
  parent: Entity          children(kind: EntityKind): [Entity!]!
  ancestors: [Entity!]!
  outgoing(kind: String): [Relation!]!
  incoming(kind: String): [Relation!]!
  props: JSON
}

type Relation { kind: String!  from: Entity!  to: Entity!
                provenance: Provenance!  source: SourceAnchor }
```

Resolvers map 1:1 onto §6, which is why the navigation API is the contract
to stabilise first. Later: `analysis(name, args)` fields, subscriptions for
incremental rebuilds.

## 9. Migration of Structure

Each step is one PR, Structure's tests and goldens staying green:

1. **Model v0.1** — `model-core`, `model-detect` (this skeleton), CI, tag.
2. **model-scip + model-link** — move `structure-scip` (loader, indexer
   runner, document cache, `link`) onto `model-core` types.
3. **model-treesitter** — the query-driven extractor; port the Java and TS
   extractors of `structure-lang-java`/`-ts` to `model.scm` + enrichers;
   compare with Structure's output on the fixtures (same entities/edges).
4. **Pharo** — tonel grammar binding, `model.scm`, sends enricher; compare with
   `structure-lang-pharo`.
5. **Structure switches** — depends on Model by git tag; a thin adapter
   converts `Model` to its IR, then classify/matrix/impact move to `Model`
   directly and `structure-ir`'s `Graph` is removed. Language detection in
   Structure uses `model-detect` (gaining multi-language roots).
6. **model-cli**, **model-analysis**, **model-graphql**.

## 10. Open questions

- Entity keys for tree-sitter-only languages: a per-language qualified name
  scheme, or SCIP-like descriptors (`<lang> . <namespace>/<Type>#<method>().`)
  so keys look the same with or without an indexer? Leaning SCIP-like.
- Keep every reference occurrence, or deduplicate per `(from, to, kind)` with
  a count? Occurrences are needed for "where is it used" in GraphQL; memory
  on large Java projects to be measured.
- Persisted format beyond JSON (bincode, SQLite) for big models and the
  GraphQL server.
- WASM: which crates must stay WASM-compatible (Structure's browser
  extension parses without SCIP).
