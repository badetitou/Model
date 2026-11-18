# Model — design

Status: draft. Done: `model-core` (metamodel, keys, compact storage),
`model-detect`, `model-scip` (loader), `model-link` (SCIP fusion and
SCIP-only models), `model-treesitter` (Java, TypeScript, Python),
`model-index` (indexer runs, document cache), the `model` facade and
`model-graphql`.

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
  ├── model-detect      language detection, project roots        ✅
  ├── model-treesitter  generic extractor driven by queries      ✅
  ├── model-scip        SCIP loading                             ✅
  ├── model-index       indexer runs, staleness, document cache  ✅
  ├── model-link        SCIP → builder (fusion, SCIP-only)       ✅
  ├── model-lang-*      optional per-language enrichers          ⏳
  └── model-analysis    Analysis trait + generic analyses        ⏳
        ^
  model (facade)   `model::build(root, &BuildOptions, progress)`  ✅
                   detect → index → extract → link
        ^
  model-cli        `model detect|build|query <path>`             ⏳
  model-graphql    GraphQL plugin (part 2)                      ✅
```

Dependencies only point up to `model-core`. Grammar crates live behind Cargo
features (`lang-java`, `lang-python`, ...; `default = ["all-languages"]`) so a
client only compiles the grammars it needs (Structure's WASM build in
particular).

## 3. Metamodel (`model-core`)

```
Model
 ├── entities
 │     id        EntityId (dense index, local to the model)
 │     key       stable unique key, SCIP-like (§3.1)
 │     kind      Namespace | File | Class | Interface | Trait | Enum | Struct
 │               | TypeAlias | Method | Constructor | Function
 │               | Field | Property | Constant | Variable
 │     name      simple name
 │     parent    containment (Namespace ⊃ Type ⊃ Method ...)
 │     language  open id: "java", "python", "pharo"...
 │     external  referenced, not defined in the sources (libraries)
 │     file, range, name_range
 │     props     open map: "java.annotations", "visibility", "http.endpoints"...
 └── relations   one per distinct (from, kind, to, provenance)
       kind        Calls | References | Inherits | Implements | Imports | Other(String)
       from, to    EntityId
       provenance  TreeSitter | Scip | Frontend | Derived
       sites       every occurrence range, in `from`'s file
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
  syntactic guess (`TreeSitter`) or only resolved facts (`Scip`). The same
  pair asserted by tree-sitter and by SCIP gives two relations.
- **Every occurrence is kept**, as a site of its relation. A site is
  attributed to its innermost enclosing entity, so its file is the file of
  `from` and is not stored. Aggregation (dependency weight, matrix cells) is
  an analysis concern: `sites().len()` is the weight.

### 3.1 Keys

Keys follow the [SCIP symbol grammar](https://github.com/sourcegraph/scip/blob/main/scip.proto)
(`model-core::SymbolKey`: build, parse, print):

```
<scheme> <manager> <package> <version> <descriptors>
semanticdb maven maven/org.acme/app 1.0 com/acme/A#run().     ← from scip-java
model      java  .                  .   com/acme/A#run().     ← from tree-sitter only
local 42                                                      ← file-local
```

- Entities resolved by an indexer keep the SCIP symbol as is.
- Frontends without an indexer use the scheme `model`, the language as
  manager and empty (`.`) package and version; descriptors are built with the
  same rules (`Descriptor::for_kind`: `ns/`, `Type#`, `method().`, `term.`).
- `SymbolKey::same_descriptors` compares keys ignoring scheme and package:
  how the linker meets a syntactic entity and its SCIP symbol, and how a
  model built without an indexer stays comparable to one built with it.
- Files are `Namespace` descriptors holding the escaped path
  (`` model java . . `src/A.java`/ ``).
- Overloads: `Method.disambiguator` (SCIP's `(+1)`); tree-sitter frontends
  derive it from the arity when needed.

### 3.2 In-memory storage

Keeping every occurrence makes sites the bulk of a model (millions on a
large Java codebase), so the frozen `Model` is laid out for them:

| Data | Layout | Cost |
|---|---|---|
| strings (keys, names, files) | interned once, one buffer + offsets | bytes + 8 / distinct string |
| entities | fixed rows, sentinel-encoded options | 52 B |
| props | sparse, sorted `(id, props)` | only where present |
| key lookup | ids sorted by key, binary search | 4 B / entity |
| children | CSR by parent | 8 B / entity |
| relations | deduplicated, sorted by `(from, kind, to, provenance)`; outgoing = one slice (CSR) | 12 B + 4 B incoming index + 4 B site offset |
| sites | bare `Range`, grouped by relation (CSR), sorted, deduplicated | 16 B |

Navigation goes through copyable views (`EntityRef`, `RelationRef`) that
borrow the model; nothing is allocated on read. `ModelBuilder` stays simple
(hash maps for dedup) and `build()` sorts and packs once.

Measured with `cargo run --release -p model-core --example memory`
(220k entities, 5M call sites, worst case: every site its own relation):
**~200 MiB, 42 B/site all included**, build + freeze in ~1.4 s.
`Model::memory()` reports the figures for real models. JSON of the same
model: 460 MiB, a binary format is on the roadmap (§10).

Further savings if needed: delta-encode sites within a relation (lines are
close), `u16` columns, memory-mapped binary format.

### 3.3 JSON

```json
{ "entities": [{ "id": 2, "key": "model java . . com/acme/A#run().",
                 "kind": "method", "name": "run", "parent": 1,
                 "language": "java", "file": "src/A.java",
                 "range": [3,2,9,3], "name_range": [3,14,3,17] }],
  "relations": [{ "kind": "calls", "from": 2, "to": 7,
                  "provenance": "scip", "sites": [[5,4,5,10],[6,4,6,10]] }] }
```

Loading goes through the builder: unknown or duplicate ids are errors.

Mapping from Structure's IR: `Symbol` → `Entity` (`SymbolKind` maps 1:1;
`annotations`, `supertypes`, `http`, `http_calls`, `rpc_calls` → `props`),
`Symbol.package` → parent `Namespace`, `Edge` → `Relation`
(`Extends` → `Inherits`, `Uses` → `References`, `Http`/`Rpc` → `Other`),
`ext:<Name>` targets → `external` entities. Structure's edges carry no
sites today; the linker will record them.

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
2. **Index** (`model-index`, done). For each detected language with an
   indexer, one run per outermost project root (directory holding a build
   marker; no marker, no run): `scip-java` and `scip-typescript` (versions
   pinned as in Structure), `scip-python`, `scip-go`, `rust-analyzer` (from
   `PATH`). Ported from Structure: process runner (progress, timeout, output
   tail, Maven/Gradle hints), staleness (`Reindex::Never|IfMissing|IfStale|Always`),
   tsconfig discovery and Node heap. Failures and missing indexers are
   warnings: the language falls back to tree-sitter relations.
   - An index records the tool that wrote it; it is only used, or
     overwritten, by that indexer. Two indexers in one root write
     `index.<indexer>.scip` (vscode has a scip-typescript `index.scip` at a
     root that also holds Java and Go files).
   - Document cache: after a run, each document is stored under
     `<git-common-dir>/<app>/scip-docs/<indexer@version>/`, keyed by the
     file's blob and path; a clean checkout of another commit composes its
     index from it without running the indexer (`from_cache`). On
     jpetstore, a fresh worktree is rebuilt from the cache (45 of 46 files)
     into the same model in 47 ms.
   - Indexes written in a sub-project have their paths prefixed
     (`backend/` + `src/A.java`).
3. **Extract** (`model-treesitter`, done). Generic tree-sitter extractor (§5).
4. **Link** (`model-link`, done). Port of Structure's `link.rs` onto the
   builder:
   - SCIP definitions are matched to existing entities by identifier line
     and name (or by extent, for scip-java's annotation-line definitions);
     matched entities take the SCIP symbol as key (`ModelBuilder::rekey`).
   - Unmatched definitions follow `Unmapped`: `Lift` attributes them to their
     nearest matched ancestor (Structure's behaviour); `Create` makes
     entities from them (kind from SCIP `kind` or the descriptor, extent from
     `enclosing_range`, parent from `enclosing_symbol` or the descriptor
     chain, created as `Namespace`s when missing) plus one `File` entity per
     document.
   - References become `Calls` (callable target), `References`, or `Imports`
     (SCIP import role), from their innermost enclosing entity, **every site
     kept**, `Provenance::Scip`. Symbols defined outside the index become
     `external` entities with their ancestor chain (`java/util/List#` under
     `java/util/` under `java/`); references to project symbols without
     entity are dropped and counted.
   - `is_implementation` relationships become `Implements`, or `Inherits`
     when a type "implements" a class or struct (SCIP does not tell
     `extends` apart).
   - Not done here: propagating calls to interface methods onto their
     implementations (Structure does it while linking); it is a `Derived`
     analysis.
5. **Build**. Cross-language relations (HTTP, RPC) are *not* built here: they
   are analyses producing `Derived` relations (§7).

Degraded modes are first class:

| Available | Result |
|---|---|
| grammar + indexer | full model (`Scip` relations) |
| grammar only | entities + syntactic relations (`TreeSitter`, names resolved best-effort) |
| indexer only | `model_link::from_scip`: entities from definitions + resolved relations, no syntactic props |

Indexer-only, measured with
`cargo run --release -p model-link --example scip_model -- <index.scip>`:

| Fixture | Index | Model | Sites at file level | Time |
|---|---|---|---|---|
| jpetstore (scip-java) | 1.1 MB, 8.7k refs | 1k entities, 3.9k relations, 5.7k sites, 0.4 MiB | 6% | 30 ms |
| angular-realworld (scip-typescript) | 0.8 MB, 5.8k refs | 0.8k entities, 1.9k relations, 3.8k sites, 0.2 MiB | 67% | 10 ms |
| vscode (scip-typescript) | 1 GB, 7.2M refs | 333k entities, 2.1M relations, 4.5M sites, 178 MiB | 47% | 4.3 s, 1.65 GB peak RSS |

The "file level" column is the limit of SCIP alone: scip-java gives the
extent of 97% of its definitions, the scip-typescript version used gives it
for 9–12%, so most TypeScript references can only be attributed to their
file. Tree-sitter extents (§5) fix that: they are what `Lift` links onto.

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

Implemented in `model-treesitter`; captures, rules and measurements are in
[its README](../crates/model-treesitter/README.md). In short:

- Captures map to entity kinds (`@definition.class` → `Class`, ...); `@name`
  gives `name_range`. Model adds `@namespace`, `@relation.inherits|implements|imports`,
  `@reference.type` and `@prop.<key>`; plain `tags.scm` files work as is
  (Python uses its grammar's, unchanged).
- **Containment is inferred from extent nesting**, so queries stay flat.
- Keys are SCIP-like, with the indexer's constructor names and overload
  numbering, so that keys stay comparable with SCIP's.
- Relations are resolved by name, best effort (`TreeSitter` provenance).
  With an index, extraction skips them (`relations: false`) and
  `model-link` (`Lift`) resolves them exactly. Definitions, extents and
  props still come from tree-sitter.
- **Enrichers** (`model-lang-*`, not started) are the escape hatch for facts
  a query cannot express (Java/TS HTTP endpoints, GWT RPC, Pharo send
  resolution). They receive the tree and the builder and only add
  props/relations.

So a new language needs a grammar crate, a query file (often just its
`tags.scm`) and a row in `grammars.rs`; optionally a SCIP indexer row in
`model-detect`.

| Language | Grammar | Query | SCIP indexer | Status |
|---|---|---|---|---|
| Java | tree-sitter-java | `java.scm` | scip-java | ✅ |
| TypeScript/TSX | tree-sitter-typescript | `typescript.scm` | scip-typescript | ✅ |
| Python | tree-sitter-python | its `tags.scm` | scip-python | ✅ baseline |
| Pharo (Tonel) | tree-sitter-tonel-smalltalk (see below) | to write | none — `Frontend` enricher | ⏳ |
| Go, Rust, C/C++, C#, Ruby, PHP | official grammars | `tags.scm` | scip-go, rust-analyzer, scip-clang, scip-dotnet, scip-ruby, scip-php | ⏳ a row each |

Tree-sitter extents are what fix SCIP-only attribution: on
angular-realworld, references at file level go from 67% to the code that
really is outside any named definition (test callbacks 54%, imports 9%,
top-level config 4%).

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

`Model`: `entity(id)`, `by_key(key)`, `entities()`, `of_kind(kind)`,
`roots()`, `relations()`, `languages()`, `memory()`.
`EntityRef`: `key`, `kind`, `name`, `language`, `external`, `file`, `range`,
`name_range`, `props`, `parent`, `ancestors`, `children`, `outgoing`,
`incoming`, `targets(kind)`.
`RelationRef`: `kind`, `from`, `to`, `provenance`, `sites`.
Lookups return `Option`/iterators and never panic.

Next additions, driven by the first analyses: `descendants`, filtered
iterators by language/file, a `ModelView` (sub-model restricted to a scope:
package, files changed in a diff), site → entity lookup by file and position.

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

## 8. Plugins — GraphQL (part 2) ✅

`model-graphql` (async-graphql 7 + axum 0.8) serves a model, built by
`model::build` or loaded from an exported `model.json`:

```sh
cargo run --release -p model-graphql -- <root | model.json> [--port 8000]
# GraphiQL on http://127.0.0.1:8000/, API on /graphql
```

The full schema is [`crates/model-graphql/schema.graphql`](../crates/model-graphql/schema.graphql)
(a test keeps it in sync). In short:

```graphql
type Query {
  languages: [String!]!
  stats: Stats!
  entity(key: String, id: Int): Entity
  file(path: String!): Entity
  entities(kind, language, name, nameContains, external, filePrefix, roots,
           first, after): EntityConnection!
  relations(kind: String, provenance: Provenance, first, after): RelationConnection!
}
type Entity {
  id key kind name language external file range nameRange props: JSON
  parent ancestors
  children(kind, first, after): EntityConnection!
  outgoing(kind: String, provenance, first, after): RelationConnection!
  incoming(kind: String, provenance, first, after): RelationConnection!
}
type Relation { id kind from to provenance siteCount sites(first, after): RangeConnection! }
```

- Resolvers hold an `Arc<Model>` and an id and map onto §6; nothing is
  copied. Relation kinds are strings (`calls`, ..., or custom like `http`).
- Every list is a forward connection (`first` default 50, at most 1000;
  `after` cursor; `totalCount`). Query depth is limited to 16.
- Read-only: the model is immutable while served.

Example, "who calls the methods of AccountService" (jpetstore):

```graphql
{ entities(name: "AccountService", kind: CLASS) { nodes {
    methods: children(kind: METHOD) { nodes {
      name
      callers: incoming(kind: "calls") { nodes { siteCount from { name parent { name } } } }
} } } } }
```

On vscode (450k entities, 2.1M relations, 4.2M sites, served from 197 MiB),
queries answer in 1–8 ms: a `nameContains` search over every entity, all
`calls` relations counted, or a class with its 157 methods and their caller
counts (4 ms).

Later: `analysis(name, args)` fields once `model-analysis` exists,
subscriptions for incremental rebuilds.

## 9. Migration of Structure

Each step is one PR, Structure's tests and goldens staying green:

1. **Model v0.1** — `model-core`, `model-detect`, CI and tag `v0.1.0` ✅.
   Updated to Rust 1.100 and `scip` 0.10.
2. **model-scip + model-link** — loader and linker ported ✅.
   2b. **model-index + `model` facade** ✅ — indexer runner, staleness and
   document cache ported from Structure's `pipeline/`; `model::build` runs
   the whole pipeline over every detected language.
3. **model-treesitter** — query-driven extractor ✅ (Java, TS, Python);
   linked with SCIP, it meets Structure's mapping-ratio test on its fixtures
   (1.000 jpetstore, 0.989 angular-realworld, Structure requires 0.8). Still
   to port from `structure-lang-java`/`-ts`: HTTP endpoints/calls and GWT RPC
   (enrichers), and a side-by-side comparison of entities.
4. **Pharo** — tonel grammar binding, `model.scm`, sends enricher; compare with
   `structure-lang-pharo`.
5. **Structure switches** — depends on Model by git tag; a thin adapter
   converts `Model` to its IR, then classify/matrix/impact move to `Model`
   directly and `structure-ir`'s `Graph` is removed. Language detection in
   Structure uses `model-detect` (gaining multi-language roots).
6. **model-graphql** ✅; **model-cli**, **model-analysis** to do.

## 10. Open questions

- Persisted binary format (bincode-like columns, or memory-mapped) for big
  models and the GraphQL server; JSON is 2.3× the in-memory size.
- Incremental rebuild: replace the entities and sites of changed files
  without rebuilding the whole model (the builder would need removal, or a
  per-file layering).
- WASM: which crates must stay WASM-compatible (Structure's browser
  extension parses without SCIP).

Decided: SCIP-like keys (§3.1); every occurrence kept, compact storage
(§3.2).
