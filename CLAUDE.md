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
  `serial` (JSON, loaded through the builder), `strings` (interner).
  `examples/memory.rs` measures the storage cost (design §3.2).
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
  entities/keys and resolves uses by name; `grammars.rs` registry (features
  `java`, `typescript`, `python`); `queries/*.scm`. Captures documented in its
  README. `tests/fixtures.rs` needs `MODEL_FIXTURES` (absolute path, e.g.
  Structure's `fixtures/`), skipped otherwise; `examples/build_model.rs`.
- `crates/model-lang-pharo` — direct Tonel frontend (`extract` on disk,
  `extract_sources` in memory): STON headers → classes/traits/methods,
  heritage and references, hand-resolved sends → `Calls` with
  `Provenance::Frontend`; `PharoStats` reported through `model::build`.
- `crates/model-index` — `ensure_index` (reuse / staleness / run, only the
  index's own tool), `process::run` (indexer process), `indexers.rs`
  registry (command per output file, hints), `cache.rs` (`DocumentCache`,
  git-blob keyed), `git.rs`. Port of Structure's `pipeline/{indexer,process,progress,scip_cache}`.
- `crates/model` — facade `build(root, opts, progress)`: detect → one index
  job per indexer and outermost marker root → tree-sitter (relations off
  for indexed languages) → link (`Lift`, or `Create` without grammar).
  `examples/build.rs`: defaults to `--reindex never`; never run indexers or
  builds on fixtures (the user does).
- `crates/model-graphql` — `schema.rs` (async-graphql objects holding
  `Arc<Model>` + id; forward connections via `page`), `lib.rs` (`schema`,
  `router`: GraphiQL `/`, API `/graphql`), `main.rs` (bin: build or load
  JSON, serve; `--no-cache` on fixtures). `schema.graphql` is checked by a
  test: regenerate with `UPDATE_SCHEMA=1 cargo test -p model-graphql`.
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
