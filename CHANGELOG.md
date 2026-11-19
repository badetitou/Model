# Changelog

## Unreleased

### Added

- `model-session`: new crate, demand-driven import. `Session::require(scope,
  requirement)` runs only the layers the traits need (structure, resolution,
  derived; declared in `LAYERS`, a trait needs resolution when it carries
  relations) on the scope's files (`Scope::{All, Files, Package, GitDiff}`),
  remembers per scope what it computed, and reports it (`Report`).
- `model-factory`: `BuildOptions::scope` and `syntactic_relations`;
  `in_scope`, `extract_fragments_in`.
- `model-core`: `ModelBuilder::from_model` re-opens a frozen model.
- `model-graphql`: `OnDemand` and `requirement_of` (selection set to traits),
  `--on-demand [--package <dir> | --diff <base>]`: a query on
  `methods { name }` starts no indexer.
- Relations: `declared_type`, `accesses`, `throws`, `overrides`
  (`RelationKind`) and a 1-byte `SiteFlags` column on sites (`READ`, `WRITE`,
  `RECEIVER`; `ModelBuilder::occurrence_flagged`, `RelationRef::site_flags`,
  JSON `flags`, GraphQL `Relation.flaggedSites`). `model-scip` exposes the
  read/write roles of references; `model-link` produces `accesses`,
  `declared_type` (type definitions) and `overrides` from SCIP;
  `model-treesitter` produces all four by name for Java (`@typed`,
  `@reference.declared_type|throws`, `@access.*` captures). Cost: +1 B/site
  (42.7 B/site, 203 MiB on 5M sites). `model-traits`: `Typed` and `Exceptions`
  traits, `overrides` / `accesses` carried by Invocation / Access.
- `model-traits`: new crate, traits as data (Named, WithMembers,
  WithInheritances, Invocation, Access, Modifiers, Annotations: typed props,
  relations, relation kinds carried, containment rules) generated from the
  FamixJava FM3 export (`cargo run -p model-traits --example generate`),
  plus a per-FM3-class trait table to fill kinds' `TraitMask`.
- `model-core`: `Model::of_trait` and `EntityRef::has_trait` query a kind's
  `TraitMask`.
- `model-core`: fine-grained kinds. `KindId` (1 byte in the entity row) and a
  `Metamodel` table (kind -> archetype, `TraitMask`, optional FM3 class name)
  carried by the `Model`; `EntityKind` stays the archetype. Builder:
  `define_kind`, `entity_of_kind`, `set_kind_id`, `kind_id`; views:
  `EntityRef::{kind_id, kind_name, kind_info}`, `Model::{metamodel, of_kind_id}`.
  JSON gains optional `fine_kind` and a top-level `metamodel`.
- `model-treesitter`: Java `record`, `annotation_type` and `enum_constant` are
  distinct kinds (captures `@definition.record|annotation_type|enum_constant`).
- `model-treesitter`: Java parameters, locals, type parameters, annotation
  instances and comments are entities (fine kinds `parameter`, `local_variable`,
  `type_parameter`, `annotation`, `comment` of archetype `Variable`; captures
  `@definition.<kind>`), keyed SCIP-like under their enclosing definition.
- `model-graphql`: `fineKind` and `fm3Class` fields on every entity.
- `model-fm3`: new crate reading a Moose FM3 metamodel exported as JSON
  (`FMJSONPrinter`) into typed structs: `Metamodel` with packages, classes,
  traits and properties (type, multivalued, container, derived, opposite),
  superclass and trait usage, built-in types as `Ref::Builtin`. Nested
  entities are accepted; dangling references and duplicate ids are errors.
  Checked with a subset of the FamixJava metamodel export.

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
