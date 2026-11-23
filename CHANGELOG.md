# Changelog

## 0.7.0 — 2026-11-23

Model in the browser, and the analyses visible over GraphQL. A new private
crate, `model-wasm`, builds a model from in-memory files and answers GraphQL
in the browser; `model-analysis` becomes queryable from `model-graphql`.

### Added

- `model-graphql`: entity facts `inDegree`, `outDegree`, `isUnused` and
  `inCycle` on every entity. `isUnused` means isolation (no call, reference or
  implementation in or out, nor on the members, and not external), not just a
  zero fan-in.
- `model-graphql`: `Query.analysis` with `metrics` (counts and per-kind,
  per-provenance and per-language breakdowns as JSON), `cycles` (count,
  cyclic entity count and components, biggest first, with `minSize` to leave
  out self-recursion), `unused` (a paged, kind-filterable connection) and
  `mostConnected` (ten project entities by default). Analyses run once, on
  first use, and do not mutate the model.
- `model-wasm` (new crate, `publish = false`): `Engine` builds a model from
  in-memory files — tree-sitter fragments, the Pharo frontend and the Python,
  TypeScript and Java enrichers — and answers GraphQL with the same schema as
  `model-graphql`, without a filesystem, threads or a server. `ModelSession`
  is its `wasm-bindgen` face (`add_file` / `build` / `execute` / `warnings`).
  `scripts/build-wasm.sh` builds the module with wasm-pack.

### Changed

- `model-graphql`: a default-on `server` feature isolates axum, tokio, the
  binary and the on-demand session. With `default-features = false` the crate
  is only the executable schema, which compiles to WebAssembly and is used by
  `model-wasm`.

## 0.6.0 — 2026-11-23

Framework enrichers. Three new crates add what the grammars and SCIP indexes
cannot see: HTTP endpoints and calls, and GWT RPC. The workspace goes from
fifteen to eighteen crates. The indexers are upgraded.

### Added

- `model-lang-python`: Flask / FastAPI / Django HTTP endpoints as
  `http.endpoint` entities, run by `model-factory` before linking; richer
  `queries/python.scm` (imports, decorators, fields, properties, async).
- `model-lang-java`: Spring / JAX-RS endpoints (prop `http.endpoints`) and GWT
  RPC (`rpc.*` props, relation `gwt-rpc`), run after linking.
- `model-lang-typescript`: HTTP calls (fetch, axios, Angular `HttpClient`) and
  endpoints (NestJS, Express-style) as `http.call` / `http.endpoint` entities
  and `http.calls` / `http.endpoints` relations.

### Fixed

- `model-index`: scip-java moved to `org.scip-code:scip-java` on Maven Central;
  upgraded to scip-java 0.14.2 and scip-typescript 0.6.2.
- `model-lang-java`: build-generated source trees are never treated as main
  sources.

## 0.5.0 — 2026-11-23

Pharo import without JSON. A new crate, `model-ffi`, exposes a frozen `Model`
over a C ABI so a Pharo image can build its Famix model straight from the
columns, and can even build the model from sources in-process. The workspace
goes from fourteen to fifteen crates.

### Added

- `model-ffi` (new crate, cdylib + rlib): flat columns (entities, relations,
  CSR sites, kinds, languages, `StrRef` strings pointing into the model),
  `model_entity_props_json`, and `model_char_offsets` (byte columns to 1-based
  inclusive character offsets, as Moose anchors need). Depends on `model-core`
  only; `unsafe` is confined to this crate, each block with a `// SAFETY:`.
- `model-ffi` optional feature `build` (off by default): `model_build` builds
  the model of a source tree in-process through `model-factory` and returns the
  same handle as `model_open`; `model_has_build`, `model_build_report`. No
  cache and no indexer unless asked.
- `pharo/`: Tonel sources (`ModelLibrary` UFFI binding, `ModelImporter`) and
  `BaselineOfModelMooseImporter` (groups `Minimal` and `ModelJava`). On
  java/util: 8.7 s through Moose JSON, 2.3 s from a `model.json`, 2.5 s from the
  sources in one call. Remaining differences with the JSON import are listed in
  `pharo/README.md`.

### Fixed

- `model-graphql`: the typed `methods` field no longer errors on containers
  that have methods (#18).

### Changed

- `scripts/crates.sh`: `model-ffi` is published after `model-factory`.

## 0.4.0 — 2026-11-20

Famix-like models. Model now builds models at the level of FamixJava from
tree-sitter and SCIP: fine-grained kinds, traits generated from the FM3
description of FamixJava, new relations, per-file fragments, demand-driven
import, typed GraphQL concepts, and a separate crate that exports a `Model` as
Moose JSON (and imports Moose JSON to diff against VerveineJ). The workspace
goes from ten to fourteen crates; the new ones are `model-fm3`, `model-traits`,
`model-moose` and `model-session`. Tracked in issue #1.

### Breaking

- `model-core`: `RelationKind` gains `DeclaredType`, `Accesses`, `Throws` and
  `Overrides`; exhaustive matches must handle them. Sites carry one more byte
  (`SiteFlags`), 42.7 B/site instead of 41.7.
- `model-index`: `IndexOptions` is no longer `Copy` and has a `custom` field;
  `ensure_index` takes the `custom` command; `IndexReport` has `custom: bool`.
- `model-graphql`: `schema` returns a `Result` and `ModelSchema` is the dynamic
  `Schema`; the Rust object types (`Class`, `Method`...) are gone. The schema
  is built from the model: the 15 archetype types are unchanged, every kind
  that names a GraphQL concept adds a type. `__typename` is that concept type
  when the kind has one, so `... on Class` and `classes` no longer hold
  entities with their own concept type (a Java `record` is a `Record`);
  `entities(kind: CLASS)` still matches all of them. `methods` on a container
  lists only entities typed `Method`; use `children(kind: METHOD)` for every
  kind of method.

### Added

#### Metamodel and storage (`model-core`)

- Fine-grained kinds: `KindId` (1 byte in the entity row) and a `Metamodel`
  table (kind -> archetype, `TraitMask`, optional FM3 class, optional GraphQL
  concept names) carried by the `Model`; `EntityKind` stays the archetype.
  Builder: `define_kind`, `entity_of_kind`, `set_kind_id`, `kind_id`,
  `set_kind_traits`, `set_kind_concept`; views: `EntityRef::{kind_id,
  kind_name, kind_info}`, `Model::{metamodel, of_kind_id}`. JSON gains
  optional `fine_kind` and a top-level `metamodel`.
- Traits on kinds: `Model::of_trait` and `EntityRef::has_trait`.
- Relations `declared_type`, `accesses`, `throws`, `overrides` and a 1-byte
  `SiteFlags` column on sites (`READ`, `WRITE`, `RECEIVER`;
  `ModelBuilder::occurrence_flagged`, `RelationRef::site_flags`, JSON `flags`).
- `ModelBuilder::from_model` re-opens a frozen model.

#### Extraction and linking

- `model-treesitter`: Java `record`, `annotation_type` and `enum_constant` are
  distinct kinds; Java parameters, locals, type parameters, annotation
  instances and comments are entities (fine kinds `parameter`,
  `local_variable`, `type_parameter`, `annotation`, `comment`), keyed SCIP-like
  under their enclosing definition. All four new relations are produced by name
  for Java (`@typed`, `@reference.declared_type|throws`, `@access.*`).
- Fine kinds are data (`model-treesitter/kinds.json`: archetype, traits, FM3
  class, GraphQL concept) and the default kinds of the archetypes carry their
  traits from `archetype_traits.json`, so `of_trait` / `has_trait` work on a
  model built without an indexer. TypeScript: `ts.interface`, `ts.type_alias`,
  `ts.namespace`, `ts.abstract_class`, `ts.arrow_function`, `ts.module`.
  Python: its own query (superclasses, parameters), `python.module`. See
  `docs/traits.md`.
- Per-file fragments: a serialisable `Fragment` per source file (entities,
  intra-file facts, unresolved references), cached by git blob
  (`FragmentCache`), and a model composed from the set (`FragmentSet::{insert,
  remove, compose}`, `Extractor`, `load_fragments`, `extract_fragments`).
  Models built from fragments are byte-identical to direct builds on the
  fixtures; a changed file only recomputes its own fragment.
- `model-scip` exposes the read/write roles of references; `model-link`
  produces `accesses`, `declared_type` and `overrides` from SCIP.
- `model-lang-pharo`: kinds against Famix-Smalltalk (`pharo.class`,
  `pharo.trait`, `pharo.method`, `pharo.extension_method`).

#### New crates

- `model-fm3`: reads a Moose FM3 metamodel exported as JSON (packages, classes,
  traits, properties with type, multivalued, container, derived, opposite).
- `model-traits`: traits as data (Named, WithMembers, WithInheritances,
  Invocation, Access, Modifiers, Annotations, Typed, Exceptions: typed props,
  relations carried, containment rules) generated from the FamixJava FM3
  export (`cargo run -p model-traits --example generate`), plus a
  per-FM3-class trait table to fill kinds' `TraitMask`.
- `model-session`: demand-driven import. `Session::require(scope,
  requirement)` runs only the layers the traits need (structure, resolution,
  derived; a trait needs resolution when it carries relations) on the scope's
  files (`Scope::{All, Files, Package, GitDiff}`), remembers per scope what it
  computed and reports it (`Report`). Guide: `docs/on-demand.md`.
- `model-moose`: imports Moose JSON into a `ModelBuilder` (tolerant streaming
  reader, FM3 classes to fine kinds, associations to relations with sites,
  `moose.id` kept) with an `ImportReport`, `diff(left, right)` by FM3 class,
  archetype and relation, and `export_json` / `export_path`, which write a
  `Model` as Moose JSON (streaming, `FM3` then `id` first, references only):
  kinds to FamixJava classes, containment through the container property of
  the (child, parent) pair, relations as reified associations anchored per
  site (`Invocation`, `Access` with `isWrite`, `Reference`, `Inheritance`,
  `Implementation`, `Import`), `EntityTyping`, `declaredExceptions`,
  `IndexedFileAnchor` with character offsets read from the sources. Derived
  properties, metrics and associations Famix refuses are left out and counted
  in `ExportReport`. Loads in Moose with `FamixJavaModel new
  importFromJSONStream:`. Examples `export` and `import`.

#### Facade, index, GraphQL

- `model-factory`: `BuildOptions::{scope, syntactic_relations,
  fragment_cache_dir}`, `in_scope`, `extract_fragments_in`, `default_model`
  (the bundled language data).
- `model-index`: user-supplied index command for builds `scip-java index`
  cannot drive (`make`), issue #17: `CustomIndex` / `IndexOptions::custom`
  (`set_custom`), `--index-command LANGUAGE=COMMAND` on `model-graphql` and the
  `build` example, `MODEL_INDEX_COMMAND`. Run with `sh -c` at the analysed root
  (`{root}`, `{output}`), never read from the analysed repository; the root is
  a project root without a build marker; the writer tool is not checked and the
  document cache uses the tag `custom-<hash>`.
- `model-graphql`: `fineKind`, `fm3Class` and `traits` on every entity;
  `fineKind` and `trait` filters on `entities(...)`; `Relation.flaggedSites`;
  typed concepts from data (`KindInfo::graphql_type|graphql_root`, set by
  `kinds.json`, `model-lang-pharo` and Moose imports): each named concept adds
  an object type implementing `Entity`, a root list and `…ByKey`;
  `--schema [<root | model.json>]`; `--on-demand [--package <dir> | --diff
  <base>]` (`OnDemand`, `requirement_of`): a query on `methods { name }`
  starts no indexer.

### Documentation

`docs/usage.md` and `docs/examples.md` (usage and worked examples),
`docs/on-demand.md`, `docs/traits.md`, `docs/famix.md` (Famix/Fame analysis
and conformity matrix), `docs/baseline.md` (measurements), and
`docs/verveinej-comparison.md` (Model's Moose export against VerveineJ on a
real project, with and without dependencies).

### Known limits

- The Moose export is an approximation: derived properties, metrics,
  signatures, generic arguments, modifiers and anonymous classes are not
  exported, and call receivers are kept only for `self`; `docs/verveinej-comparison.md`
  lists the measured gaps.
- Relations resolved by name (tree-sitter) ignore receivers and inheritance;
  a SCIP index is needed for exact relations.
- Per-file fragments skip parsing but a full rebuild from a warm cache is not
  faster than a direct build on very large repositories (vscode); the gain is
  skipping layers and files on demand.
- The `make` recipe for indexing a project that `scip-java index` cannot drive
  is documented but untested on the OpenJDK.

## 0.3.0 — 2026-11-18

All ten crates are now published on crates.io; git tags remain available for
pinning the whole workspace. The facade is named `model-factory` (`model` is already
taken on crates.io): in Rust, use `model_factory::build` and `model_factory::…`.

### Added

- `model-graphql`: one by-key root field per entity kind (`classByKey`,
  `methodByKey`, …, plus `entityByKey`), each with a single required
  `key: String!` argument. Introspection-driven clients such as fluid use this
  shape as a concept's by-id lookup: it enables links in generated tables and
  opens detail columns. `Class` and the other containers also expose a typed
  `methods: [Method!]!` relation, so a class's methods can be listed and
  navigated without a hand-written query.
- `model-lang-pharo`: direct Pharo/Tonel frontend. Parses `Class`, `Trait` and
  `Extension` files into `model pharo . . Name#` entities, emits `Inherits` and
  `References`, and resolves message sends into `Calls` with
  `Provenance::Frontend` and their source sites. Files, classes, methods,
  resolved/unresolved sends and emitted calls are reported as `PharoStats`.
- `model_factory::build` runs the Pharo frontend for detected Pharo projects
  and exposes its report as `Build.pharo`; the crate is re-exported as
  `model_factory::lang_pharo`. No Structure change is needed.

## 0.2.0

### Breaking — `model-graphql`

- `Entity` is now a GraphQL interface with one concrete object per
  `EntityKind`: `Namespace`, `File`, `Class`, `Interface`, `Trait`, `Enum`,
  `Struct`, `TypeAlias`, `Method`, `Constructor`, `Function`, `Field`,
  `Property`, `Constant`, `Variable`. Navigation fields keep returning
  `Entity`, so queries using only common fields still work, but `__typename`
  now returns the concrete name instead of `Entity`, and fragments can target
  a kind. The concrete types are also exported from the crate.
- `Query` gains one list field per kind (`classes`, `methods`, `interfaces`,
  …), each returning a typed connection, so introspection-driven clients see
  the kinds as distinct concepts. `entities(kind:)` and `entity(key/id:)`
  remain available and return the interface.

### Added

- `model-graphql`: `--allow-origin <origin|*>` (repeatable, comma-separated)
  enables CORS for browser clients such as Fluid. Without it the server keeps
  sending no CORS header. `router_with_origins` exposes it as a library API;
  introspection stays enabled.
- Documentation and clients now point at tag `v0.2.0`.

## 0.1.2

- `model-analysis`: language-neutral `Analysis` trait and generic analyses —
  `Metrics`, `Degree`/`Degrees`, `Cycle`/`Cycles` (iterative Tarjan),
  `Unused`, `Propagation`/`Impact`, `quotient`/`QuotientAnalysis`, and
  `InterfaceCalls` returning `Provenance::Derived` relations with their sites.
- `model` re-exports the crate as `model::analysis` and the trait as
  `model::Analysis`.
- Refreshed README and build example output; clients now point at tag `v0.1.2`.

## 0.1.1

Toolchain and dependency refresh; no API change.

- Rust 1.100, pinned by `rust-toolchain.toml`. `rust-version` and CI's MSRV
  job updated accordingly.
- `scip` 0.10 (protobuf 3.7.2) and refreshed lockfile.
- CI verified on Linux and macOS, with each grammar feature alone.

## 0.1.0

First version, extracted from [Structure](https://github.com/badetitou/Structure).

- `model-core`: language-neutral metamodel (entities, containment,
  relations with provenance and every occurrence site), SCIP-like keys,
  compact in-memory storage (~40 B per site), JSON.
- `model-detect`: languages of a tree, file counts, project roots (15
  languages).
- `model-scip`: SCIP index reader (parallel, bounded memory).
- `model-link`: SCIP onto tree-sitter entities (`Lift`), or SCIP-only models
  (`Create`).
- `model-treesitter`: query-driven extraction for Java, TypeScript/TSX and
  Python (its grammar's `tags.scm`).
- `model-index`: runs SCIP indexers (scip-java, scip-typescript,
  scip-python, scip-go, rust-analyzer), staleness policy, git-blob keyed
  document cache.
- `model`: `build(root)` runs detection, indexing, extraction and linking.
- `model-graphql`: serves a model over GraphQL, with GraphiQL.

Requires Rust 1.100.
