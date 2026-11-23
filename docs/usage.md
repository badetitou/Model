# Usage guide

The reference for the APIs and the servers of the 0.5.0 release: what each
layer gives you, how to call it, and its limits. For worked scenarios with
output you can reproduce, see [examples.md](examples.md).

## Which document for what

| Document | Read it to... |
|---|---|
| [README](../README.md) | understand what Model is, build a first model, see the crates |
| **usage.md** (this page) | look up an API: kinds, traits, relations, sessions, GraphQL, fragments, Moose, index command |
| [examples.md](examples.md) | follow nine end-to-end scenarios on a sample project (`docs/examples/shop`), with commands, output and pitfalls |
| [on-demand.md](on-demand.md) | understand on-demand import: which query costs which layer, scopes, caching, troubleshooting |
| [traits.md](traits.md) | see which traits the kinds of each language carry |
| [famix.md](famix.md) | compare Model with FamixJava / Fame (analysis written before epic #1: its "Gap" tables are history) |
| [design.md](design.md) | storage layout, pipeline, measurements, design decisions |
| [baseline.md](baseline.md) | the measurements taken before epic #1 |
| [verveinej-comparison.md](verveinej-comparison.md) | the measured comparison of Model's Moose export with VerveineJ's, on VerveineJ's own sources |

The Rust snippets below are kept compiling by
`crates/model-moose/tests/usage.rs` and `crates/model-session/tests/usage.rs`;
the runnable versions are the examples of [examples.md](examples.md)
(`cargo test --workspace` runs them all).

**Defaults to know.** The command-line tools (`build`, `model-graphql`) default
to `--reindex never`: they use an existing `index.scip` and never start an
indexer. The *library* default (`BuildOptions::default()`,
`IndexOptions::default()`) is `Reindex::IfStale`: a project root with a build
marker and no up-to-date index **will** start its indexer. Set
`opts.index.reindex = Reindex::Never` when embedding Model in a service.

## 1. Overview

| Layer | Crate | What it gives you |
|---|---|---|
| Fine kinds | `model-core` | `EntityKind` (15 archetypes) stays; each entity also has a 1-byte `KindId` into a `Metamodel` table (`record`, `annotation_type`, `enum_constant`, `parameter`, `local_variable`, `type_parameter`, `annotation`, `comment`, `ts.interface`, ...) with archetype, trait mask and optional FM3 class |
| New relations | `model-core` | `RelationKind::{DeclaredType, Accesses, Throws, Overrides}`; each site has a 1-byte `SiteFlags` (`READ`, `WRITE`, `RECEIVER`) |
| FM3 reader | `model-fm3` | `Metamodel::from_path` reads a Moose FM3 metamodel exported as JSON |
| Traits | `model-traits` | traits as data, generated from the FM3 of FamixJava (`Named`, `WithMembers`, `WithInheritances`, `Invocation`, `Access`, `Modifiers`, `Annotations`, `Typed`, `Exceptions`); `mask`, `mask_of_fm3_class`; `Model::of_trait` / `EntityRef::has_trait` |
| Fragments | `model-treesitter`, `model-index` | one serialisable `Fragment` per file, cached by git blob; the model is composed from fragments |
| Session | `model-session` | `require(scope, requirement)`: only the layers the requested traits need, on the scope's files |
| Moose | `model-moose` | export a `Model` as Moose JSON, import Moose JSON, diff report |

```
sources ─ tree-sitter ─► Fragment (per file, cached) ─┐
                                                      ├─► ModelBuilder ─► Model ─► model-moose ─► Moose JSON
SCIP index (optional) ─ model-link ───────────────────┘        │            ▲
                                                               ▼            │
            model-session: require(scope, traits)          GraphQL     import_json
            traits ─► layers (structure | resolution | derived)
            scope  ─► files  (All | Files | Package | GitDiff)
```

## 2. Build a Java model, query by kind and trait

```rust
use model_core::{EntityKind, ModelBuilder};
use model_treesitter::{ExtractOptions, extract_sources};

let mut b = ModelBuilder::new();
extract_sources(
    &mut b,
    &[("app/Base.java", BASE), ("app/App.java", APP)], // (relative path, text)
    &ExtractOptions::default(),
)?;
let model = b.build();

// By archetype, or by fine kind (a row of the metamodel).
let classes = model.of_kind(EntityKind::Class);
let params = model.of_kind_id(model.metamodel().find("parameter").unwrap());
```

From a directory (and, with a SCIP index, compiler-grade relations):
`cargo run --release -p model-factory --example build -- <root> [--no-scip]`,
or `model_factory::build(root, &BuildOptions::default(), &|_| {})`.

**Traits.** A kind carries a `TraitMask`; `model_traits::mask(&["WithMembers"])`
builds one, `Model::of_trait(mask)` / `EntityRef::has_trait(mask)` test it.
Tree-sitter models carry traits without any indexer: the default kinds of the
archetypes get theirs from `crates/model-treesitter/archetype_traits.json`
(class: `Named`, `WithMembers`, `WithInheritances`, `Invocation`, `Modifiers`,
`Annotations`; method: `Named`, `Invocation`, `Access`, `Modifiers`,
`Annotations`, `Typed`, `Exceptions`; field, namespace...), and the fine kinds
(`record`, `parameter`, `local_variable`...) from `kinds.json`. Both are data.

```rust
let with_members = model_traits::mask(&["WithMembers"]);
let types: Vec<_> = model.of_trait(with_members).map(|e| e.name()).collect();
let throwing = model.of_trait(model_traits::mask(&["Exceptions"])); // methods, constructors
```

A kind can also be defined from an FM3 class (Moose imports do it
automatically):

```rust
let record = b.define_kind(
    "record",
    EntityKind::Class,
    model_traits::mask_of_fm3_class("Famix-Java-Entities.Class"),
    Some("Famix-Java-Entities.Record"),
);
```

## 3. Read the new relations

```rust
use model_core::{RelationKind, SiteFlags};

let run = model.of_kind(EntityKind::Method)
    .find(|m| m.name() == "run" && m.parent().is_some_and(|p| p.name() == "App"))
    .unwrap();
let overridden = run.targets(&RelationKind::Overrides);          // App.run -> Base.run
let access = run.outgoing().find(|r| *r.kind() == RelationKind::Accesses).unwrap();
let f = access.site_flags()[0];                                   // `count += 1`
assert!(f.contains(SiteFlags::READ) && f.contains(SiteFlags::WRITE));
let thrown = name_method.targets(&RelationKind::Throws);          // method -> exception type
let typed = field.targets(&RelationKind::DeclaredType);           // typed entity -> its type
```

| Relation | From → to | Site | Flags |
|---|---|---|---|
| `declared_type` | field, parameter, local, method return → type | type position | none |
| `accesses` | member → field / parameter / local | the identifier | `READ`, `WRITE`, `RECEIVER` |
| `throws` | method → exception type | `throws` clause | none |
| `overrides` | method → overridden/implemented method | none | none |

`sites()` and `site_flags()` are parallel slices. In GraphQL,
`Relation.flaggedSites` returns each occurrence with its flags
(`flags: ["read","write"]`, `read`, `write`, `receiver`). Provenance
matters: SCIP gives `accesses` (when the indexer reports read/write roles),
`declared_type` and `overrides`; `throws`, parameters/locals accesses and the
rest come from tree-sitter, by name (`Provenance::TreeSitter`).

## 4. Sessions and GraphQL

This section is the quick start; the full guide to on-demand import (what
triggers which layer, scopes, caching, limits, troubleshooting) is
[on-demand.md](on-demand.md).

```rust
use model_session::{Requirement, Scope, Session};

let mut s = Session::new("path/to/repo");
// Entities only: structure layer, no SCIP index read, no indexer started.
let p = s.require(&Scope::Package("src/main/java/app".into()),
                  &Requirement::traits(&["Named", "WithMembers"]))?;
println!("{} entities, layers run: {:?}", p.model.entity_count(), p.report.layers_run);
// A trait that carries relations (Invocation, Access...) adds the resolution layer.
let p = s.require(&Scope::All, &Requirement::traits(&["Invocation"]))?;
```

To check that no indexer ran: `p.report.indexing_events == 0` (the indexing
step never looked for an index) and `p.report.indexers_started == 0`
(processes actually spawned). Runnable: `cargo run -p model-session --example sessions`.

Scopes: `All`, `Files(..)`, `Package(dir)`, `GitDiff { base }`. A session
remembers what it computed per scope: the same or fewer traits return the
stored model (`report.layers_run` is empty), more traits rebuild with the
union. `Requirement::with_analysis("interface-calls")` adds the only derived
analysis registered so far.

From GraphQL, `--on-demand` lets each query's selection set decide:

```bash
cargo run --release -p model-graphql -- path/to/repo --on-demand            # whole root
cargo run --release -p model-graphql -- path/to/repo --on-demand --package src/main/java/app
cargo run --release -p model-graphql -- path/to/repo --on-demand --diff origin/main
```

`{ methods { nodes { name } } }` starts no indexer; following
`outgoing(kind: "calls")` adds the resolution layer, which *uses* an existing
`index.scip` and starts an indexer only with `--reindex if-stale|if-missing|always`
(the server defaults to `never`). `OnDemand` and `requirement_of` (selection → traits) are in
`crates/model-graphql/src/ondemand.rs`. Use `--no-cache` on fixtures.

### Fine kinds, traits and typed concepts in GraphQL

`model-graphql` carries no language knowledge. Its schema is built from the
model at startup (async-graphql's dynamic schema): the 15 archetype types
(`Class`, `Method`, `Variable`, ...) are always there, and every kind of the
model's `Metamodel` that **names a GraphQL concept** gets one more object type,
implementing `Entity`, with a list field and a lookup by key:

```graphql
{
  parameters(first: 5) { totalCount nodes { name traits parent { name } } }  # type Parameter
  parameterByKey(key: "...") { name }
  tsInterfaces { nodes { name } }                    # TypeScript, from kinds.json
  pharoClasses { nodes { name children(kind: METHOD) { totalCount } } }
}
```

- A concept is a root list `parameters(first, after)` (a `ParameterConnection`)
  and `parameterByKey(key: String!)`. The by-key name is the type name,
  lower-cased first letter, plus `ByKey`.
- Concept types exist **when the model has the kind**: they appear as the
  frontend meets the kind (a Java file without parameters defines no
  `parameter` kind), and `--schema <root | model.json>` prints the schema of a
  given model. `--schema` alone prints the one of the bundled language data
  (`crates/model-graphql/schema.graphql`).
- `__typename` of an entity is its kind's concept type when the kind has one,
  otherwise the archetype type. So `... on Class` and the `classes` list hold
  the class-archetype entities that have **no** concept type of their own (a
  Java `record` is a `Record`, not a `Class`); `entities(kind: CLASS)` still
  matches all of them. `methods(first)` on containers lists only entities typed
  `Method`; use `children(kind: METHOD)` for every kind of method.
- Generic access stays available for every kind, with or without a concept:
  `Entity.fineKind: String!` (exact kind name, `record`, `parameter`, ...),
  `Entity.traits: [String!]!` (`Named`, `Typed`, `Invocation` ...), and
  `entities(fineKind: "parameter", trait: "Invocation", ...)` (an unknown trait
  is an error listing the known ones; an unknown `fineKind` is just an empty
  list). A kind without a concept name keeps its
  archetype type.
- `--on-demand`: typed roots and by-key fields need only the structure layer;
  no indexer runs unless the query follows relations.

A name is dropped, and the kind keeps its archetype type, when it is not a
valid GraphQL name or is already taken (by an archetype, `Query` fields, an
earlier kind); a root name already taken only drops the list field.

#### How a language declares its concepts

Each kind carries two optional strings in the `Metamodel`
(`KindInfo::graphql_type`, `KindInfo::graphql_root`); they are serialised in
model JSON, so a saved model keeps its concepts.

- **tree-sitter languages**: add the two fields to the kind's row in
  `crates/model-treesitter/kinds.json`; no Rust change:

  ```json
  {"name": "ts.interface", "archetype": "interface",
   "graphql_type": "TsInterface", "graphql_root": "tsInterfaces",
   "traits": ["Named", "WithMembers"]}
  ```

- **Rust frontends** (`model-lang-*`): call the builder when defining the kind:

  ```rust
  let k = b.define_kind("pharo.class", EntityKind::Class, mask, Some("Famix-Smalltalk-Entities.Class"));
  b.set_kind_concept(k, Some("PharoClass"), Some("pharoClasses"));
  ```

- **Moose imports**: derived from the FM3 class (`Famix-Java-Entities.Record`
  → `JavaRecord`, `javaRecords`).

Archetype default kinds cannot carry a concept (`set_kind_concept` returns
false): they already have their archetype type.

### Indexing a project the built-in indexers cannot build

(Scenario g of [examples.md](examples.md) shows the failure messages.)

`scip-java index` only drives Gradle, Maven and Bazel, and a project root needs
a build marker to be indexed at all. For a `make`-built project (or any custom
pipeline) say yourself how its index is produced:

```bash
# flag (repeatable, one per language) or environment variable
cargo run --release -p model-graphql -- path/to/repo \
  --reindex if-stale --index-command 'java=./tools/make-index.sh {root} {output}'
MODEL_INDEX_COMMAND='java=make -C {root} scip' cargo run --release -p model-factory --example build -- path/to/repo
```

```rust
let mut opts = BuildOptions::default();
opts.index.set_custom(CustomIndex::new("java", "./tools/make-index.sh {root} {output}"));
```

- **Only you can set it.** The command comes from the flag, `MODEL_INDEX_COMMAND`
  or `IndexOptions::custom`; it is **never read from a file of the analysed
  repository**, so a repository cannot run commands on whoever analyses it.
  Do not forward untrusted text into it.
- **`LANGUAGE=COMMAND`.** The language (a `model-detect` id with a SCIP indexer:
  `java`, `typescript`, `python`, `go`, `rust`...) is required, there is no
  implicit default: it names the indexer being replaced, so `java=` also
  replaces scip-java for `kotlin` and `scala`. Other languages keep their normal
  indexers. A flag replaces the environment variable for the same language.
- **How it runs.** `sh -c <command>` in the project root with the usual timeout
  (`MODEL_INDEXER_TIMEOUT`, default 600 s, killed on expiry), progress from its
  output and the tail of its output on failure. `{root}` is the absolute project
  root (also the working directory) and `{output}` the file name the index must
  have in it (`index.scip`, or `index.scip-java.scip` when another indexer shares
  the root). Both are replaced by **shell-quoted** values: write them bare, not
  inside quotes (`"{root}"` would nest quotes). Scripts can read the same values
  from `$MODEL_INDEX_ROOT` and `$MODEL_INDEX_OUTPUT`. It needs `sh` (no Windows
  without a POSIX shell).
- **Roots.** The analysed root itself is the project root for that language,
  with or without a build marker, and nested markers do not start the built-in
  indexer in that run. With `--reindex never` an existing `{output}` there is
  used and nothing runs.
- **Reuse and staleness** are unchanged: `never`, `if-missing`, `if-stale`
  (default for the library: stale = a source file of the language, outside
  `build/`, `target/`, `node_modules/`..., newer than the index) and `always`.
- **Which tool wrote the index** is *not* checked for a custom command: it is
  yours and may legitimately be scip-java's, or another tool's. (Built-in
  indexers still refuse an index written by another tool.) Document cache keys
  use a distinct stable tag `custom-<hash of the command text>`, so they are
  never mixed with `scip-java-0.12.3` documents; changing the command string
  starts a new cache.
- **Failures** are warnings and the language falls back to tree-sitter:
  program not found (exit 127, shown with the command), non-zero exit (tail of
  the output), exit 0 but no `{output}` written, timeout (`MODEL_INDEXER_TIMEOUT`).

**Recipe for a `make`-built Java project — UNTESTED on the OpenJDK.** scip-java
can index an arbitrary build by hand: add its javac plugin to every `javac`
call (`-Xplugin:scip -sourceroot:<root> -targetroot:<dir>`, plus whatever
`--add-exports` flags the plugin needs), then run `scip-java aggregate
<targetroot>` to write `index.scip`. Write that in
your own script, for example `tools/make-index.sh`:

```sh
#!/bin/sh
# usage: make-index.sh <root> <output>      (run by you, adapt to your build)
root=$1; output=$2; target=$root/.scip-targetroot
rm -rf "$target"; mkdir -p "$target"
# 1. rebuild with the semanticdb javac plugin injected (build-specific!)
make -C "$root" JAVAC_EXTRA_FLAGS="-Xplugin:scip -sourceroot:$root -targetroot:$target" || exit 1
# 2. turn the SemanticDB files into the index
scip-java aggregate --output "$root/$output" "$target"
```

```bash
cargo run --release -p model-graphql -- ~/jdk --reindex if-missing \
  --index-command 'java=./tools/make-index.sh {root} {output}'
# or build the index once yourself, then only reuse it:
cargo run --release -p model-graphql -- ~/jdk --reindex never
```

Caveats: not tried on the OpenJDK. Its build runs its own `javac` invocations
per module (the bootstrap `javac` and interim compilers, which may not accept
the plugin), the flag name for extra options above is a placeholder, and the
scip-java manual-configuration guide describes a newer scip-java than the one
pinned here (`SCIP_JAVA_VERSION`, 0.12.3): check the plugin and
subcommand names (`-Xplugin:scip` or `-Xplugin:semanticdb`, `aggregate` or
`index-semanticdb`) against `scip-java --help` of the version you use. Safer alternatives: `--no-scip` (tree-sitter entities and name-resolved
relations, nothing to run), or `--on-demand --package <dir>` to model only part
of the tree.

## 5. Fragments and incremental rebuild

Each source file yields a `Fragment` (JSON-serialisable) keyed by its git
blob; a changed file recomputes only its own fragment, then the model is
composed again from the set.

```rust
use model_factory::index::FragmentCache;
use model_factory::extract_fragments;
use model_factory::treesitter::ExtractOptions;

let cache = FragmentCache::at(".git/model-fragments".into());
let (set, stats, _warning) = extract_fragments(root, &ExtractOptions::default(), Some(&cache))?;
// first run: stats.extracted = N; unchanged files later: stats.reused = N
let mut b = ModelBuilder::new();
set.compose(&mut b, true);            // true = relations by name
```

`BuildOptions::fragment_cache_dir` (or the default under
`<git-common-dir>/<cache>/fragments`) does the same inside `model_factory::build`
and `Session`. Blob ids are hashed from the files on disk (uncommitted edits count), and an
explicit `FragmentCache::at(dir)` works anywhere, git or not. Only the default
location, `<git-common-dir>/<cache>/fragments`, needs a git repository: outside
one there is simply no cache. If the blob ids cannot be computed,
`extract_fragments` returns a warning and parses everything. Lower level:
`Extractor`, `load_fragments`, `FragmentSet::{insert, remove, compose}`.

## 6. Moose JSON

**Export and load in Moose.**

```rust
use model_moose::{ExportOptions, export_path};
let report = export_path(&model, "model.json", &ExportOptions::default())?;
print!("{report}");   // objects written, skipped entities/relations by reason
```

`ExportOptions { root, no_sources }`: `root` resolves relative file names to
read the sources for character-offset anchors; `no_sources` writes no anchors.
From a Model JSON file: `cargo run --release -p model-moose --example export -- model.json out.json [--root DIR]`.
In a Pharo image with Moose:

```smalltalk
model := FamixJavaModel new.
'out.json' asFileReference readStreamDo: [ :s | model importFromJSONStream: s ].
```

(The export of the sample project of [examples.md](examples.md) was loaded in a
Moose image for the 0.4.0 documentation: 253 objects, equal to the export
report, 10 classes, 16 methods, 5 invocations, 31 accesses. Earlier, a jpetstore
export was accepted by a Moose image too.)

**Import and diff.** Import a Moose JSON (e.g. VerveineJ output) and compare it
with a model built from the same sources:

```rust
use model_moose::{ImportOptions, diff, import_json};
let mut b = ModelBuilder::new();
let imported = import_json(&mut b, &bytes, &ImportOptions::default());
let moose = b.build();                     // keys `moose:<id>`, props `moose.*`
let report = diff(&moose, &model);        // rows by FM3 class, archetype, relation kind
```

A real comparison (VerveineJ on its own sources) is in
[verveinej-comparison.md](verveinej-comparison.md); a reproducible walk-through
on a small project is scenarios e and f of [examples.md](examples.md).

Command line: `cargo run --release -p model-moose --example import -- moose.json [--root DIR] [--no-sources] [--diff model.json]`.
`ImportOptions::with_fm3(&fm3_metamodel)` adds the containment properties a
newer FM3 metamodel flags as `container`.

## 7. Regenerate the traits from an FM3 export

`crates/model-traits/traits.json` maps each Model trait to the FM3 traits it
aggregates; `src/generated.rs` is generated.

```bash
# export the FamixJava FM3 from Moose as JSON, then:
cargo run -p model-fm3 --example summary -- famixjava-fm3.json       # sanity check
cargo run -p model-traits --example generate -- famixjava-fm3.json   # rewrites src/generated.rs
cargo test -p model-traits                                            # fails when generated.rs is stale
```

Without an argument, `generate` reads the trimmed FamixJava subset fixture of
`model-fm3`. Edit `traits.json` to add or regroup traits.

## 8. Known limits

Taken from the crates' docs and commit messages; nothing here is new
research.

- **Traits are per archetype, not per language**: `class`, `method`... carry
  the same Famix-like mask for Java, TypeScript and Python (a TypeScript method
  has `Exceptions` too). Only the fine kinds are language-specific. The FM3
  class of those default kinds is not set (it would be Java-only); the Moose
  export maps them by kind name. Sessions use traits only to choose layers.
- **Name-based relations** (tree-sitter): receivers and inheritance are
  ignored (`a.save()` resolves only when `save` is unique), overloads are not
  told apart for `overrides`, generics and lambdas are out of scope. Use a SCIP
  index for exactness.
- **Accesses** read/write come from SCIP only when the indexer sets a role,
  else the use stays a `references`.
- **Session**: Pharo is always read whole; growing the layers of a scope
  rebuilds (the structure is recomposed from fragments, not kept); only one
  derived analysis (`interface-calls`); per request fixed cost ~28 ms of query
  compilation, so skipping files gains little on small projects (design §4.1);
  the GraphQL field table is Java/Famix based, unknown fields mean all traits.
- **Fragments**: on vscode a warm rebuild (7.6–10.6 s) is slower than a direct
  parse (3.9 s): the JSON fragments are 219 MB and every blob is hashed;
  composing is a full pass. A binary format is a follow-up.
- **Moose export** is an approximation: the receiver of a call is not kept, so
  only `self` receivers are generated; derived properties, metrics,
  `previous`/`next`, signatures (unless imported), generic arguments are not
  written; `overrides` is derived in Famix and omitted; associations Famix
  refuses are skipped (counted in `ExportReport`); kinds with no FM3 class
  (files, comments, functions, plain variables) are not exported. Class `Exception`
  promotion and `Access.variable` are workarounds learned from a Moose image.
- **Moose import**: real VerveineJ exports can be invalid strict JSON (raw
  control characters); the reader is tolerant.
- **Other languages**: TypeScript/Python/Pharo have few fine kinds;
  FM3 class names for TypeScript and Smalltalk were written from the Famix
  repositories and **not** checked against an export; Pharo trait usage,
  extensions, stubs and metaclasses are missing ([traits.md](traits.md)).
- [famix.md](famix.md) §3 is the analysis made *before* the epic: its "Gap"
  tables describe what was missing then.
