# Model

[![CI](https://github.com/badetitou/Model/actions/workflows/ci.yml/badge.svg)](https://github.com/badetitou/Model/actions/workflows/ci.yml)

A language-neutral model of a codebase, built from
[tree-sitter](https://tree-sitter.github.io/) (syntax) and
[SCIP](https://github.com/sourcegraph/scip) (resolved symbols), with
automatic language detection. Analyses and plugins (GraphQL) work on the
model, not on a language.

Extracted from [Structure](https://github.com/badetitou/Structure).
Design: [`docs/design.md`](docs/design.md).

## Crates

| Crate | Status | Role |
|---|---|---|
| `model-core` | usable | metamodel, SCIP-like keys, `ModelBuilder`, compact `Model` keeping every occurrence, JSON |
| `model-detect` | usable | languages of a tree, file counts, project roots |
| `model-scip` | usable | reads `index.scip` (parallel, bounded memory) |
| `model-link` | usable | SCIP → model: fusion onto syntactic entities, or SCIP-only models |
| `model` | usable | `model::build(root, …)`: detect → index → extract → link, every language at once |
| `model-index` | usable | runs SCIP indexers (staleness, timeout, hints), caches documents per file content |
| `model-treesitter` | usable | query-driven extraction (Java, TypeScript, Python); [captures](crates/model-treesitter/README.md) |
| `model-graphql` | usable | serves a model over GraphQL (GraphiQL included); [schema](crates/model-graphql/schema.graphql) |

## Use

```toml
[dependencies]
model = { git = "https://github.com/badetitou/Model", tag = "v0.1.1" }
```

```rust
let build = model::build(root, &model::BuildOptions::default(), &|p| {
    eprintln!("[{:>3}%] {}", p.pct, p.step)
})?;
for w in &build.warnings {
    eprintln!("warning: {w}");
}
let model = build.model;
let svc = model.by_key("semanticdb maven … com/acme/AccountService#").unwrap();
for r in svc.outgoing() {
    println!("{:?} {} ({} sites)", r.kind(), r.to().key(), r.sites().len());
}
```

The individual crates are re-exported (`model::core`, `model::detect`,
`model::index`, `model::scip`, `model::link`, `model::treesitter`) for
finer control, e.g. a model from an index alone:
`model::link::from_scip(&model::scip::load(path)?, None)`.

Serve it over GraphQL: `cargo run --release -p model-graphql -- <root>`,
then open GraphiQL on http://127.0.0.1:8000/.

Try it: `cargo run --release -p model --example build -- <root>` (add
`--reindex if-stale` to run indexers, `--json model.json` to export).

## Development

Rust 1.100 (pinned by `rust-toolchain.toml`). CI (`.github/workflows/ci.yml`) runs the commands below
on Linux and macOS, each grammar feature alone, and the minimum Rust version.


- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

## License

MIT
