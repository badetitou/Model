# Changelog

## Unreleased

- `model-graphql`: `--allow-origin <origin|*>` (repeatable, comma-separated)
  enables CORS for browser clients such as Fluid. Without it the server keeps
  sending no CORS header. `router_with_origins` exposes it as a library API;
  introspection stays enabled.

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
