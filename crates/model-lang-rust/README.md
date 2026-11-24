# model-lang-rust

Rust frontend for [Model](../../README.md), built from the SCIP index produced
by `rust-analyzer scip . --output index.scip`.

Rust has no tree-sitter grammar in Model yet. `build` therefore links the index
with `model_link::Unmapped::Create`: every definition becomes an entity keyed by
its full SCIP symbol, references become `Calls`/`References`/`Imports` with
their occurrence sites, and one `File` entity is created per document.

`enrich` adds the facts rust-analyzer only encodes in symbol names, since its
SCIP `relationships` are empty:

- `impl#[Point][Draw]` blocks become entities of fine kind `rust.impl`
  (declared in `kinds.json`), parent their methods, and carry
  `rust.self_type`, `rust.trait` and `rust.kind = "trait_impl"`. Inherent
  `impl#[Point]` blocks carry `rust.kind = "impl"` and no `rust.trait`.
- A trait implementation adds `Implements` from the type to the trait
  (`Provenance::Frontend`) and `Overrides` from each impl method to the
  matching trait method.
- Enum variants keep the `rust.enum_variant` kind instead of being ordinary
  constants, and module definitions become `Namespace` entities marked with
  `rust.kind = "module"`.

All keys remain the original SCIP symbols, so enrichment merges with a model
already built by `model-factory` (detect → index → link) and is idempotent.

## Usage

```rust
let index = model_scip::load(std::path::Path::new("index.scip"))?;
let (builder, stats) = model_lang_rust::build(&index);
let model = builder.build();
println!("{} impls, {} overrides", stats.impls, stats.overrides);
```

`model-factory` runs `enrich` automatically for every Rust index; the facts
are reported in `Build::rust`. `declare_kinds` is called by
`model_factory::default_model()`.

## Tests

`cargo test -p model-lang-rust` uses synthetic in-memory indexes. To also check
a real index:

```sh
MODEL_RUST_INDEX=/path/to/index.scip cargo test -p model-lang-rust
```
