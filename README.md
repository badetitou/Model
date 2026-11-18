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
| `model-detect` | skeleton | languages of a tree, file counts, project roots |
| `model-treesitter`, `model-scip`, `model-link` | planned | extraction and fusion |
| `model-graphql` | planned | GraphQL plugin |

## Use

```toml
[dependencies]
model-core = { git = "https://github.com/badetitou/Model" }
model-detect = { git = "https://github.com/badetitou/Model" }
```

```rust
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
