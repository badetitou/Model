# Changelog

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
