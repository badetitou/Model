# Model

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
| `model-treesitter` | planned | query-driven syntactic extraction |
| `model-graphql` | planned | GraphQL plugin |

## Use

```toml
[dependencies]
model-core = { git = "https://github.com/badetitou/Model" }
model-detect = { git = "https://github.com/badetitou/Model" }
```

```rust
// A model from a SCIP index alone.
let index = model_scip::load("index.scip".as_ref())?;
let (builder, stats) = model_link::from_scip(&index, None);
let model = builder.build();

// Languages of a tree.
let detection = model_detect::detect(std::path::Path::new("."))?;
for l in &detection.languages {
    println!("{} — {} files, roots {:?}", l.spec.name, l.files, l.project_roots);
}
```

## Development

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

## License

MIT
