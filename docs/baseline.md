# Baseline measurements (issue #5, epic #1)

Reference numbers taken **before** any metamodel change. Later performance
checks compare against this file.

## Conditions

- Model commit: `c131f33` (0.3.0), `cargo build --release`, default features.
- Machine: macOS (Darwin 27.2), local developer laptop, warm file cache.
  Timings are wall-clock and noisy: take the median of several runs, expect
  ±20 % on the small phases (the models are tiny).
- Fixtures (Structure's `fixtures/`, `FIX=/Users/benoit.verhaeghe/Documents/Perso/Structure/fixtures`):
  - `jpetstore-6` @ `8f11fab`, Java, 46 files, existing `index.scip` (scip-java, 1.0 MiB).
  - `angular-realworld` @ `d426ec8`, TypeScript, 50 files, existing `index.scip` (scip-typescript, 0.7 MiB).
- **No indexer or build was run on the fixtures.** Only the pre-existing
  `index.scip` files were read (`--reindex never`).

## Results

Median of 3 runs (range in parentheses). "SCIP load + link" in `build_model`
includes loading the SCIP file (`model_scip::load`) and linking it.

| Metric | jpetstore-6 | angular-realworld |
|---|---|---|
| Files | 46 | 50 |
| SCIP documents / symbols / definitions / references | 45 / 651 / 1140 / 8709 | 50 / 1046 / 1463 / 5834 |
| tree-sitter extract (relations off) | 60 ms (51–62) | 50 ms (50–58) |
| SCIP load + link (`build_model`) | 18.9 ms (18–68, first run cold) | 7.4 ms (7–20) |
| `model_factory::build` total, SCIP + tree-sitter | 69 ms (56–110) | 79 ms (76–95) |
| `model_factory::build` total, `--no-scip` (tree-sitter only, relations by name) | 78 ms | 44 ms |
| Entities (of which external) | 978 (352) | 761 (400) |
| Relations | 3861 | 1920 |
| Sites (occurrences kept) | 5671 | 3827 |
| Sites attributed to a whole file | 368 (6.5 %) | 2562 (66.9 %) |
| `Model::memory()` heap | 0.4 MiB | 0.2 MiB |
| Definitions mapped / mapped or lifted | 94.5 % / 100 % | 96.7 % / 98.9 % |
| `Model` freeze (`ModelBuilder::build`) | 0.7–1.0 ms | 0.5–0.8 ms |

Tree-sitter only (no index, relations resolved by name): jpetstore-6 has
752 entities, 1110 relations, 1248 sites, 0.2 MiB; angular-realworld has 373
entities, 92 relations, 214 sites, 0.1 MiB.

SCIP-only model (`scip_model`, no tree-sitter): jpetstore-6 1003 entities,
3940 relations, 5671 sites, load 24 ms + link 4.7 ms + build 0.5 ms, 0.4 MiB;
angular-realworld 792 entities, 1850 relations, 3832 sites, load 2.9 ms +
link 5.9 ms + build 0.5 ms, 0.2 MiB. Only 172/1463 angular definitions carry
an enclosing range, which explains the many file-level sites.

Not measured: the Moose/VerveineJ side (done by the user); the
`spring-boot-realworld-example-app` and `vscode` fixtures (out of scope for
this issue). Storage cost at scale (synthetic 20 000 types) is in
`crates/model-core/examples/memory.rs` (design §3.2); re-run it separately if
needed.

## Commands

```bash
FIX=/Users/benoit.verhaeghe/Documents/Perso/Structure/fixtures
cargo build --release -p model-treesitter -p model-factory -p model-link -p model-core --examples

# tree-sitter time, link time (incl. SCIP load), counts, memory
for p in jpetstore-6 angular-realworld; do
  ./target/release/examples/build_model $FIX/$p     # repeat 3x, take the median
done

# facade: whole pipeline, existing index only; add --no-scip for tree-sitter only
./target/release/examples/build $FIX/jpetstore-6 --reindex never --no-cache
./target/release/examples/build $FIX/angular-realworld --reindex never --no-cache
./target/release/examples/build $FIX/jpetstore-6 --reindex never --no-scip --no-cache

# SCIP-only model
./target/release/examples/scip_model $FIX/jpetstore-6/index.scip
./target/release/examples/scip_model $FIX/angular-realworld/index.scip

# synthetic storage cost (design §3.2)
cargo run --release -p model-core --example memory
```

`--reindex never` is the facade default; never pass another value on the
fixtures (CLAUDE.md). `--no-cache` avoids writing the per-document cache.
