# Model — notes for Claude

Language-neutral code model from tree-sitter + SCIP, with language
auto-detection. Design and roadmap: `docs/design.md`. Extracted from
Structure (`../Structure`), its first client.

## Commands

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

## Map

- `crates/model-core` — serde only. `key` (SCIP-like `SymbolKey`),
  `builder` (`ModelBuilder`: get-or-create by key, relations deduplicated,
  every occurrence site kept), `model` (frozen compact `Model`: interned
  strings, fixed rows, CSR relations/sites; views `EntityRef`/`RelationRef`),
  `serial` (JSON, loaded through the builder), `strings` (interner),
  `BRANCHES_PROP` (frontend fact behind cyclomatic complexity).
  `examples/memory.rs` measures the storage cost (design §3.2). `KindInfo` also
  carries optional `graphql_type`/`graphql_root` (concept names, set by the
  languages' data, serialised in the metamodel).
- `crates/model-scip` — `load(index.scip)` → `ScipIndex` (interned symbols,
  per-document definitions with enclosing range / references with import
  role, symbol infos with `EntityKind`); raw-document helpers for caching.
  Port of Structure's `structure-scip::scip`.
- `crates/model-link` — `link(builder, index, opts)`: match definitions to
  entities (rekey to SCIP symbol), `Unmapped::Lift|Create`, references →
  `Calls`/`References`/`Imports` with sites, externals with ancestors,
  `Implements`/`Inherits`; `from_scip` = SCIP-only model. `symbols.rs`:
  per-symbol facts computed once. `examples/scip_model.rs` runs on a real
  index (Structure's `fixtures/*/index.scip`).
- `crates/model-treesitter` — `extract(builder, root, opts)`: `facts.rs`
  runs a grammar's query (capture roles, nesting by extent), `apply.rs` makes
  entities/keys and resolves uses by name (`FragmentSet::compose`); per-file
  serialisable `Fragment` (`Extractor`, `load_fragments`, `FragmentStore`); `grammars.rs` registry (features
  `java`, `typescript`, `python`); `queries/*.scm` (`@complexity.branch` →
  `complexity.branches` int prop). Captures documented in its README.
  `tests/fixtures.rs` needs `MODEL_FIXTURES` (absolute path, e.g.
  Structure's `fixtures/`), skipped otherwise; `examples/build_model.rs`.
- `crates/model-lang-pharo` — direct Tonel frontend (`extract` on disk,
  `extract_sources` in memory): STON headers → classes/traits/methods,
  heritage and references, hand-resolved sends → `Calls` with
  `Provenance::Frontend`; `PharoStats` reported through `model::build`.
- `crates/model-lang-python` — enricher (`enrich`/`enrich_sources`) run by
  `model-factory` before linking: Flask/FastAPI/Django HTTP endpoints as
  `http.endpoint` entities (`Provenance::Frontend`). Richer
  `queries/python.scm` (imports, decorators, fields, properties, async).
- `crates/model-lang-java` — enricher run after linking: Spring/JAX-RS
  endpoints (prop `http.endpoints`) and GWT RPC (`rpc.*` props, relation
  `gwt-rpc`), ported from Structure. Extension facts: not exported to Moose.
- `crates/model-lang-typescript` — enricher run by `model-factory` after linking
  (`enrich` on disk, `enrich_sources` in memory): HTTP calls (fetch, axios,
  Angular `HttpClient`) and endpoints (NestJS, Express-style) as `http.call` /
  `http.endpoint` entities (kinds in `kinds.json`) + `http.calls`/`http.endpoints`
  relations (`Provenance::Frontend`) and owner list props. README holds the
  Model → FamixTypeScript class mapping; ported from Structure's `lang-ts/http.rs`.
- `crates/model-lang-rust` — rust-analyzer SCIP frontend (`build`/`from_scip`,
  `enrich` from the index after linking): modules, `rust.impl` blocks with
  `rust.self_type`/`rust.trait`, `Implements`/`Overrides` and `rust.enum_variant`
  kinds from symbol names (`kinds.json` concepts `RustImpl`/`rustImpls`,
  `RustEnumVariant`/`rustEnumVariants`); no grammar
- `crates/model-index` — `ensure_index` (reuse / staleness / run, only the
  index's own tool; none for a custom one), `custom.rs` (`CustomIndex`: user-only
  `sh -c` index command, never read from the analysed repo; `--index-command
  LANGUAGE=COMMAND`, `MODEL_INDEX_COMMAND`, cache tag `custom-<hash>`; a custom
  command makes the analysed root a project root), `process::run` (indexer process), `indexers.rs`
  registry (command per output file, hints), `cache.rs` (`DocumentCache`,
  `FragmentCache`, git-blob keyed), `git.rs`. Port of Structure's `pipeline/{indexer,process,progress,scip_cache}`.
- `crates/model-factory` (crate `model-factory`) — facade `build(root, opts, progress)`: detect → one index
  job per indexer and outermost marker root → tree-sitter (relations off
  for indexed languages) → link (`Lift`, or `Create` without grammar).
  `examples/build.rs`: defaults to `--reindex never`; never run indexers or
  builds on fixtures (the user does).
- `crates/model-cli` — bin `model` (`main.rs` dispatch, `commands.rs`, `load.rs`, `output.rs`):
  `detect`, `build` (atomic `--out`, default `--reindex never`), `query`,
  `context` (entity + callers + callees in one call), `impact`, `analysis`
  (`metrics|code-metrics|cycles|unused|degrees`); JSON on stdout, progress on stderr,
  `MODEL_TIMING=1` reports the per-invocation load of `model.json` (measured
  ~1.9 s and ~1.1 GB peak RSS on a 212 MB export). Tests in `tests/cli.rs`
  drive the binary with `CARGO_BIN_EXE_model` and write fixtures under unique
  temp dirs. Stateless by design; a long-lived client should hold the model in
  memory rather than reload it.
- `crates/model-analysis` — read-only `Analysis` over a frozen `Model`:
  `Metrics` (structural counts), `CodeMetrics` (LOC from ranges, cyclomatic
  complexity from `BRANCHES_PROP`), degrees, cycles, unused, impact, quotient,
  `InterfaceCalls`. GraphQL and Moose export use the same helpers.
- `crates/model-session` — `Session::require(&Scope, &Requirement)`: traits →
  layers (`layers.rs`: structure / resolution / derived, declarative, a trait
  needs resolution iff it carries relations), scope → files
  (`All|Files|Package|GitDiff`), per-scope memo, `Report` (layers run,
  indexing events). Uses `BuildOptions::scope`. `examples/measure.rs`.
- `crates/model-graphql` — `schema.rs` (async-graphql *dynamic* schema built
  from the model: `ARCHETYPES` table, `Entity` interface from `ENTITY_SPECS`,
  one object type + root list + `…ByKey` per kind whose `KindInfo` names a
  `graphql_type`/`graphql_root`; invalid or clashing names fall back to the
  archetype type), `lib.rs` (`schema(model)`, `router`: GraphiQL `/`, API
  `/graphql`), `main.rs` (bin: build or load JSON, serve; `--schema [input]`;
  `--no-cache` on fixtures). `ondemand.rs` (`OnDemand`, `requirement_of`:
  selection set → traits → session; an unknown root field is a typed concept,
  structure layer only; `--on-demand`). No language or kind name in `src/` (a
  test checks): concepts come from data, `kinds.json`/`define_kind` (issue #16).
  `schema.graphql` is checked by a test (bundled language data via
  `model_factory::default_model`): regenerate with
  `UPDATE_SCHEMA=1 cargo test -p model-graphql`.
- `crates/model-fm3` — `Metamodel::from_json_str|from_path`: typed reader for
  Moose FM3 metamodel JSON (packages, classes, traits, properties, `Ref`);
  fixture `tests/fixtures/famixjava-subset.json` (real export, trimmed).
- `crates/model-traits` — traits as data (`TraitDef`: name, bit, props,
  relations, `carries`, containment), `generated.rs` produced from
  `traits.json` + the FamixJava FM3 export by `examples/generate.rs`
  (`cargo run -p model-traits --example generate`); `mask`, `trait_def`,
  `mask_of_fm3_class` feed `Metamodel::define`, then `Model::of_trait` /
  `EntityRef::has_trait` (core only sees opaque `TraitMask` bits). A test
  fails when `generated.rs` is stale.
- `crates/model-moose` — Moose JSON import (`import_json`/`import_path` →
  `ImportReport`; tolerant streaming reader `json`, FM3→kind/relation tables
  in `mapping`; entities keyed `moose:<id>`, props `moose.id`/`moose.fm3`/
  `moose.<prop>`) and `diff(left, right)` → `DiffReport` by FM3 class,
  archetype and relation. Depends on core, analysis, traits, fm3.
  `examples/import.rs`; `MOOSE_JSON=file cargo test -p model-moose` runs a
  real export. Export: `export_json`/`export_path` (`export.rs`, streaming,
  hand-written JSON; class/container/domain tables in `mapping`; LOC and
  cyclomatic complexity computed by `model-analysis`; anchors need
  the source files, see the module doc); `tests/export.rs` validates every
  property against the full FamixJava FM3 fixture and round-trips through the
  importer. `examples/export.rs`. Acceptance in Moose: load with
  `FamixJavaModel new importFromJSONStream:`.
- `crates/model-ffi` — C ABI (cdylib + rlib) over a frozen `Model` for foreign
  importers (Pharo UFFI). Depends on `model-core` + `model-analysis` (+ `serde_json`
  to open a `model.json`): no language, no Famix. Optional feature `build` (off by
  default; test it with `cargo test -p model-ffi --features build`) adds
  `model_build`: sources → handle in-process via `model-factory`, no JSON, no
  cache, no indexer unless asked; hence `model-ffi` is published after
  `model-factory`. `model_open` → handle; flat
  columns (entities, relations, CSR sites, kinds/languages, `StrRef` strings
  pointing into the model); `model_entity_props_json`; `model_entity_metrics`
  and `model_entity_metrics_batch` (LOC + cyclomatic complexity);
  `model_char_offsets` (byte columns → 1-based inclusive char offsets, Moose
  anchors). `unsafe` is
  allowed here only, each block with `// SAFETY:` (user's agreement, 2026-11).
  `KindInfo.fm3` is empty for models built from sources: the client maps kind
  names to FM3 classes. Pharo side: the separate repository
  [Model-Moose-Importer](https://github.com/badetitou/Model-Moose-Importer) holds
  the Tonel sources (`ModelLibrary` UFFI binding, `ModelImporter`,
  `BaselineOfModelMooseImporter` depending on
  `github://moosetechnology/Famix:development/src`, groups `Minimal` +
  `ModelJava`); parity with the JSON import checked on a small Java project.
  Never read columns with
  `FFIExternalArray fromPointer:type:size:` (crashed the image): copy with
  `LibC memCopy:to:size:`.
- `crates/model-detect` — `detect(root)`: languages, file counts, project
  roots; `languages.rs` is the data table (`LanguageSpec`).

## Rules

1. Before calling a change done: fmt, clippy `-D warnings` and tests pass.
2. No `unwrap()`/`expect()` outside tests and `main`.
3. No `unsafe` without a `// SAFETY:` comment and the user's agreement.
4. New dependencies only when justified; grammars behind Cargo features.
5. Keep `EntityRow`/`RelRow`/`Range` sizes (test `rows_stay_compact`);
   re-run `examples/memory` after storage changes.
6. Language-specific knowledge goes in data (language table, query files) or
   frontends/enrichers (`model-lang-*`), never in `model-core`.
7. Commit only when asked.

## Releasing

The twenty crates listed in `scripts/crates.sh` are publishable on crates.io;
`model-wasm` has `publish = false`. Keep the workspace version, the internal path
dependency versions in `[workspace.dependencies]`, `CHANGELOG.md` and the
version references in `README.md` and `docs/` in sync. The facade is
`model-factory` because `model` is taken on crates.io.

To release: bump those versions, add a `## X.Y.Z — date` section to
`CHANGELOG.md`, commit, then `git tag -a vX.Y.Z -m vX.Y.Z` and push the tag.
The `release` job in `.github/workflows/ci.yml` waits for lint/test/features/msrv,
checks that the tag matches `Cargo.toml`, publishes every crate with
`scripts/publish.sh` (crate list and order in `scripts/crates.sh`), then creates
a GitHub Release whose notes come from `scripts/release-notes.sh`.

The repository needs a `CARGO_REGISTRY_TOKEN` secret with publish rights on all
twenty crates. Local equivalent: `./scripts/publish.sh`. Re-running a tag fails:
crates.io versions are immutable.
