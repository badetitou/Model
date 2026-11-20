<p align="center">
  <img src="https://raw.githubusercontent.com/badetitou/Model/main/assets/logo.svg" width="120" alt="Model logo" />
</p>

<h1 align="center">Model</h1>

<p align="center"><strong>A language-neutral model of your codebase.</strong></p>

<p align="center">
Syntax from <a href="https://tree-sitter.github.io/">tree-sitter</a>, resolved
symbols from <a href="https://github.com/sourcegraph/scip">SCIP</a>: one graph
that every analysis and plugin can share — whatever the language.
</p>

<p align="center">
  <a href="https://github.com/badetitou/Model/actions/workflows/ci.yml"><img src="https://github.com/badetitou/Model/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://crates.io/crates/model-factory"><img src="https://img.shields.io/crates/v/model-factory.svg?style=flat-square" alt="crates.io"></a>
  <a href="https://github.com/badetitou/Model/tree/main/crates"><img src="https://img.shields.io/badge/crates-14-6EE7B7?style=flat-square" alt="14 crates"></a>
  <a href="https://github.com/badetitou/Model/blob/main/rust-toolchain.toml"><img src="https://img.shields.io/badge/rust-1.100%2B-A78BFA?style=flat-square&logo=rust&logoColor=white" alt="Rust 1.100+"></a>
  <a href="https://github.com/badetitou/Model/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-0B1020?style=flat-square" alt="MIT license"></a>
</p>

Model builds **one model per codebase, not one per language**. Java, TypeScript,
Python and Pharo sources land in the same graph of entities and relations, and
so will every future language: detection, indexing and extraction are
data-driven. A language usually is a grammar, a query file and a row in a table;
when neither grammar nor indexer exists, a direct frontend fills the same
builder.

```
Namespace ⊃ Class ⊃ Method
                    ├── calls ─────► Method
                    ├── references ► Field
                    └── implements ► Interface
```

<p align="center"><sub>
<a href="#highlights">Highlights</a> ·
<a href="#how-it-works">How it works</a> ·
<a href="#quick-start">Quick start</a> ·
<a href="#use-it-as-a-library">Library</a> ·
<a href="#crates">Crates</a> ·
<a href="#languages">Languages</a> ·
<a href="#numbers">Numbers</a> ·
<a href="#graphql-in-action">GraphQL</a> ·
<a href="#development">Development</a> ·
<a href="docs/usage.md">Usage guide</a>
</sub></p>

## Highlights

- **Nothing to configure.** Languages, file counts and project roots are
  detected from the tree, `.gitignore` included. Monorepos just work.
- **Best of both worlds.** tree-sitter gives fast, error-tolerant syntax and
  exact extents; SCIP indexers give compiler-grade symbol resolution. One
  without the other is a supported, first-class mode.
- **Every occurrence is kept.** A relation is unique per
  `(from, kind, to, provenance)` and stores *all* its sites, so dependency
  weight, impact of a diff or a heat map needs no re-parse.
- **Compact and fast.** ~43 bytes per site all included (range + 1-byte flags), 5M sites in ~200 MiB;
  reads borrow the model and never allocate.
- **Stable identity.** Entities are keyed by SCIP symbols (or SCIP-like keys for
  languages without an indexer), so two builds are comparable.
- **Batteries included.** GraphQL server with GraphiQL, JSON export, a git-blob
  keyed document cache that rebuilds an index without running the indexer.

## How it works

```mermaid
flowchart LR
  D["detect<br/>languages · roots"]
  I["index<br/>scip-java · scip-typescript · …"]
  E["extract<br/>tree-sitter + queries"]
  F["frontend<br/>Pharo/Tonel reader"]
  L["link<br/>SCIP symbols onto entities"]
  M[("Model")]
  A["analyses"]
  G["GraphQL"]

  D --> I --> L
  D --> E --> L
  D --> F --> L
  L --> M
  M --> A
  M --> G
```

1. **Detect** — scan the tree, count source files, find project roots (build
   markers such as `pom.xml`, `package.json`, `Cargo.toml`…).
2. **Index** — run (or reuse) the SCIP indexer of each project, with staleness
   policies, timeouts, progress and a per-document cache.
3. **Extract** — tree-sitter queries produce entities, extents, containment and
   language-specific props. Relations are resolved by name when no index exists.
3b. **Frontend** — languages without a grammar are read directly; Pharo/Tonel
   classes, methods and hand-resolved message sends feed the same builder with
   `Provenance::Frontend`.
4. **Link** — SCIP definitions are matched to syntax entities (rekeyed to the
   SCIP symbol); references become `calls`, `references` and `imports` with every
   site, `Provenance::Scip`.

Failures are warnings, never dead ends: no indexer → syntactic relations; no
grammar → SCIP-only model; neither → a direct frontend may still model the
language (Pharo), otherwise the other languages are modelled.

## Quick start

<p align="center">
  <img src="https://raw.githubusercontent.com/badetitou/Model/main/assets/demo.gif" alt="Building the model of a Java project" width="100%">
</p>

<p align="center"><sub>
Model on Structure's <code>jpetstore</code> fixture: 46 Java files, already indexed by
scip-java. Regenerate the recording with <code>vhs docs/demo.tape</code>.
</sub></p>

```sh
git clone https://github.com/badetitou/Model && cd Model
```

**Build the model of any project** — and see what it holds:

```sh
cargo run --release -p model-factory --example build -- ../Structure
```

Example output on jpetstore (46 Java files):

```
language java — 46 files, project roots ["."]
tree-sitter java (46 files, 571 definitions, 0 unresolved): relations from SCIP
scip-java in . (reused): 0 created, 94.5% mapped (100.0% with lifting), 5671 sites
978 entities (141 types, 525 callables, 352 external)
3861 relations, 5671 sites, 0.4 MiB, languages ["java"], in 51.4 ms
```

Useful flags:

| Flag | Effect |
|---|---|
| `--reindex never \| if-missing \| if-stale \| always` | when to run indexers (default: `never`) |
| `--no-scip` | tree-sitter only: entities + best-effort relations |
| `--from-cache` | compose the index from cached documents instead of running the indexer |
| `--no-cache` | do not read or write the document cache |
| `--json model.json` | export the model |

**Explore it over GraphQL** (GraphiQL included):

```sh
cargo run --release -p model-graphql -- ../Structure --reindex if-stale
# GraphiQL on http://127.0.0.1:8000/ , API on /graphql
```

The server also accepts an exported `model.json` instead of a source root, and
`--schema` prints the SDL. Introspection is enabled (GraphiQL uses it). For a
browser client on another origin — e.g. Fluid on `http://localhost:5173` —
allow it explicitly:

```sh
cargo run --release -p model-graphql -- ../Structure \
  --allow-origin http://localhost:5173
```

`--allow-origin` is repeatable, accepts comma-separated origins, and `*` allows
any origin. Without it, no CORS header is sent.

## Use it as a library

```toml
[dependencies]
model-factory = "0.3.0"
```

```rust
let build = model_factory::build(root, &model_factory::BuildOptions::default(), &|p| {
    eprintln!("[{:>3}%] {}", p.pct, p.step);
})?;
for w in &build.warnings {
    eprintln!("warning: {w}");
}

let model = build.model;
let svc = model.by_key("semanticdb maven maven/org.acme/app 1.0 com/acme/AccountService#")
    .expect("AccountService is indexed");

for r in svc.outgoing() {
    println!("{:?} {} ({} sites)", r.kind(), r.to().key(), r.sites().len());
}
```

Need finer control? Every crate is re-exported (`model_factory::core`, `model_factory::detect`,
`model_factory::index`, `model_factory::scip`, `model_factory::link`, `model_factory::treesitter`), so you can
build a model from an index alone:

```rust
use std::path::Path;

let index = model_factory::scip::load(Path::new("index.scip"))?;
let (mut builder, stats) = model_factory::link::from_scip(&index, None);
println!("{} calls, {} imports", stats.calls, stats.imports);
let model = builder.build();
```

### Navigating a model

`Model` exposes `entity(id)`, `by_key(key)`, `entities()`, `of_kind(kind)`,
`roots()`, `relations()`, `languages()` and `memory()`.
`EntityRef` gives `key`, `kind`, `name`, `language`, `external`, `file`,
`range`, `props`, `parent`, `ancestors`, `children`, `outgoing`, `incoming` and
`targets(kind)`. Lookups return `Option` or iterators; nothing panics.

```rust
use model_factory::{EntityKind, Model, RelationKind};

fn print_call_counts(model: &Model, class_name: &str) {
    let Some(class) = model.entities().find(|e| e.name() == class_name) else {
        return;
    };
    for method in class.children().filter(|e| e.kind() == EntityKind::Method) {
        let sites: usize = method
            .incoming()
            .filter(|r| r.kind() == &RelationKind::Calls)
            .map(|r| r.sites().len())
            .sum();
        println!("{} — {sites} call site(s)", method.name());
    }
}
```

## The metamodel

```
Entity
  id          dense index, local to the model
  key         stable identity: SCIP symbol or SCIP-like key
  kind        Namespace | File | Class | Interface | Trait | Enum | Struct
              | TypeAlias | Method | Constructor | Function
              | Field | Property | Constant | Variable
  name        simple name
  parent      containment (Namespace ⊃ Type ⊃ Method …)
  language    open id: "java", "typescript", "python"…
  external    referenced but not defined in the sources
  file, range, name_range
  props       open map: "java.annotations", "visibility", "http.endpoints"…

Relation
  kind        Calls | References | Inherits | Implements | Imports | Other(String)
  from, to    entities
  provenance  TreeSitter | Scip | Frontend | Derived
  sites       every occurrence range, in from's file
```

The contract is deliberately small: **closed entity kinds, open everything
else.** Analyses can match `EntityKind` exhaustively; language-specific facts
go into `props`, and plugins can add relations under their own name.

## Crates

| Crate | Role |
|---|---|
| [`model-core`](https://github.com/badetitou/Model/tree/main/crates/model-core) | metamodel, SCIP-like keys, `ModelBuilder`, compact `Model`, JSON |
| [`model-detect`](https://github.com/badetitou/Model/tree/main/crates/model-detect) | languages of a tree, file counts, project roots |
| [`model-scip`](https://github.com/badetitou/Model/tree/main/crates/model-scip) | reads `index.scip` (parallel, bounded memory) |
| [`model-link`](https://github.com/badetitou/Model/tree/main/crates/model-link) | SCIP → model: fusion onto syntactic entities, or SCIP-only models |
| [`model-treesitter`](https://github.com/badetitou/Model/tree/main/crates/model-treesitter) | query-driven extraction; [captures](https://github.com/badetitou/Model/blob/main/crates/model-treesitter/README.md) |
| [`model-index`](https://github.com/badetitou/Model/tree/main/crates/model-index) | runs SCIP indexers, staleness, document cache |
| [`model-lang-pharo`](https://github.com/badetitou/Model/tree/main/crates/model-lang-pharo) | direct Tonel frontend: classes, methods, references, resolved sends |
| [`model-analysis`](https://github.com/badetitou/Model/tree/main/crates/model-analysis) | `Analysis` trait: metrics, degrees, cycles, unused, quotient, impact, derived relations |
| [`model-factory`](https://github.com/badetitou/Model/tree/main/crates/model-factory) | facade: detect → index → extract → link; `model_factory::analysis` re-export |
| [`model-session`](https://github.com/badetitou/Model/tree/main/crates/model-session) | demand-driven import: `require(scope, traits)` runs only the layers needed |
| [`model-fm3`](https://github.com/badetitou/Model/tree/main/crates/model-fm3) | reads Moose FM3 metamodels exported as JSON |
| [`model-traits`](https://github.com/badetitou/Model/tree/main/crates/model-traits) | traits as data, generated from the FamixJava FM3; `has_trait` / `of_trait` |
| [`model-moose`](https://github.com/badetitou/Model/tree/main/crates/model-moose) | Moose JSON import, diff report, and export; see the [usage guide](docs/usage.md) |
| [`model-graphql`](https://github.com/badetitou/Model/tree/main/crates/model-graphql) | GraphQL server with GraphiQL; [schema](https://github.com/badetitou/Model/blob/main/crates/model-graphql/schema.graphql) |

Grammar crates sit behind Cargo features (`java`, `typescript`, `python`;
`default = ["java", "typescript", "python"]`), so a client only compiles the
grammars it needs — WASM builds included.

## Languages

| Language | Grammar | Query | Indexer | Status |
|---|---|---|---|---|
| Java | tree-sitter-java | `java.scm` | scip-java | ✅ |
| TypeScript / TSX | tree-sitter-typescript | `typescript.scm` | scip-typescript | ✅ |
| Python | tree-sitter-python | its `tags.scm` | scip-python | ✅ baseline |
| Pharo (Tonel) | — (direct reader) | — | — | ✅ frontend |
| Go, Rust, C/C++, C#, Ruby, PHP | official grammars | `tags.scm` | scip-go, rust-analyzer, scip-clang, scip-dotnet, scip-ruby, scip-php | ⏳ a row each |

Adding a language means a grammar crate, a query file — often its own
`tags.scm` — and a row in `model-treesitter`'s grammar registry, or a small
`model-lang-*` frontend when the language has no usable grammar or indexer.

## Numbers

Measured on real projects (see [`docs/design.md`](https://github.com/badetitou/Model/blob/main/docs/design.md) for the
method):

| Fixture | Model | Time | Notes |
|---|---|---|---|
| jpetstore (46 Java files) | 978 entities, 3.9k relations, 5.7k sites | ~50 ms | 94.5% of SCIP definitions mapped, 100% with lifting |
| angular-realworld (50 TS files) | 0.8k entities, 1.9k relations, 3.8k sites | 50 ms | references attributed to code: 67% SCIP alone → 98.9% linked |
| vscode (15.7k TS files) | 333k entities, 2.1M relations, 4.5M sites, 197 MiB | 6.5 s | served over GraphQL in 1–8 ms per query |

Storage is laid out for occurrences: interned strings, fixed rows, CSR
relations and sites, binary-search key lookup. Run
`cargo run --release -p model-core --example memory` to reproduce.

## GraphQL in action

```graphql
{
  entities(name: "AccountService", kind: CLASS) {
    nodes {
      children(kind: METHOD) {
        nodes {
          name
          callers: incoming(kind: "calls") {
            nodes { siteCount from { name parent { name } } }
          }
        }
      }
    }
  }
}
```

`Entity` is a GraphQL **interface**: every `EntityKind` has its own concrete
object (`Class`, `Method`, `Interface`, …), so `__typename` returns the real
kind and clients can use fragments. Navigation fields (`parent`, `children`,
`from`, `to`, …) still return the interface, so queries using only the common
fields keep working. Each kind also has a root field (`classes`, `methods`,
`interfaces`, …) to enumerate its instances:

```graphql
{ classes { nodes { __typename name
    ... on Class { methods: children(kind: METHOD) { totalCount } }
} } }
```

Every list is a forward connection (`first`, `after`, `totalCount`), relation
kinds are strings so plugins can introduce their own, and the model is
immutable while served. `__typename` on an entity used to be `Entity`; it is
now the concrete type name — clients matching on it must be updated.

Each kind also has a single-item lookup with one required `key` argument
(`classByKey`, `methodByKey`, `traitByKey`, …), and containers expose a typed
`methods` list. Introspection-driven clients such as fluid need both: the
list/by-key pair makes their generated tables clickable, and `methods` is the
relation a class detail view can show without hand-written queries:

```graphql
{ classByKey(key: "model pharo . . OrderService#") {
    name
    methods { key name }
} }
```

## Development

Rust 1.100, pinned by [`rust-toolchain.toml`](https://github.com/badetitou/Model/blob/main/rust-toolchain.toml). CI runs the
commands below on Linux and macOS, checks each grammar feature alone, and
enforces the MSRV.

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Fixture tests need real projects and their SCIP indexes: set `MODEL_FIXTURES`
to an absolute path (for example Structure's `fixtures/`); they are skipped
otherwise.

The demo GIF is recorded from [`docs/demo.tape`](https://github.com/badetitou/Model/blob/main/docs/demo.tape) with
[vhs](https://github.com/charmbracelet/vhs): build the example once, then run
`vhs docs/demo.tape` from the repository root.

## Roadmap

`model-analysis` is available as a generic socle (metrics, fan-in/fan-out,
cycles, unused entities, quotient, propagation, derived relations) and Pharo is
modelled by a direct frontend. Next: a `model-cli`
(`model detect|build|query <path>`), GraphQL analysis fields, a persisted binary
format for very large models, and incremental rebuilds of changed files. See
[`docs/design.md`](https://github.com/badetitou/Model/blob/main/docs/design.md).

## License

[MIT](https://github.com/badetitou/Model/blob/main/LICENSE) © 2026 Benoit Verhaeghe
