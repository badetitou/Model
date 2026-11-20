# Usage guide: FamixJava-level models

A short, practical tour of what epic [#1](https://github.com/badetitou/Model/issues/1)
added. Deep references: [design.md](design.md) (storage, layers, numbers),
[famix.md](famix.md) (Famix/Fame analysis and conformity matrix),
[traits.md](traits.md) (traits beyond Java), [baseline.md](baseline.md)
(measurements before the epic).

The Rust snippets below are kept compiling by
`crates/model-moose/tests/usage.rs` and `crates/model-session/tests/usage.rs`
(`cargo test --workspace` runs them).

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
`Relation.flaggedSites` returns each occurrence with its flags. Provenance
matters: SCIP gives `accesses` (when the indexer reports read/write roles),
`declared_type` and `overrides`; `throws`, parameters/locals accesses and the
rest come from tree-sitter, by name (`Provenance::TreeSitter`).

## 4. Sessions and GraphQL

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
`outgoing(kind: "calls")` does. `OnDemand` and `requirement_of` (selection → traits) are in
`crates/model-graphql/src/ondemand.rs`. Use `--no-cache` on fixtures.

### Famix-like concepts in GraphQL

Fine kinds are first-class in the schema. Besides the 15 archetype types
(`Class`, `Method`, `Variable`, ...), there is one concrete type per fine kind
of the default kind tables, each implementing `Entity`, with a root list field
and a `...ByKey(key: String!)` lookup:

| Java | TypeScript / Python | Pharo |
| --- | --- | --- |
| `Parameter` (`parameters`, `parameterByKey`) | `TsInterface` (`tsInterfaces`) | `PharoClass` (`pharoClasses`) |
| `LocalVariable` (`localVariables`) | `TsTypeAlias`, `TsNamespace`, `TsAbstractClass` | `PharoTrait` (`pharoTraits`) |
| `TypeParameter` (`typeParameters`) | `TsArrowFunction`, `TsModule` | `PharoMethod` (`pharoMethods`) |
| `Annotation`, `AnnotationType` (`annotations`, `annotationTypes`) | `PythonModule` (`pythonModules`) | `PharoExtensionMethod` |
| `Record` (`records`), `EnumConstant` (`enumConstants`), `Comment` (`comments`) | | |

- `__typename` of an entity is its fine kind's type when the kind is in that
  list (a parameter is a `Parameter`, a record a `Record`, a Pharo class a
  `PharoClass`), otherwise its archetype's type. Kinds defined elsewhere
  (user `kinds.json`, Moose imports) keep the archetype type; `fineKind`
  always gives the exact name. So `... on Class` no longer matches records or
  Pharo classes: use `... on Record` or filter on `kind: CLASS`, which still
  matches every class-archetype entity. `kind`, `classByKey`, `classes` ... keep
  working on the archetype.
- `Entity.traits: [String!]!` lists the trait names of the entity's kind
  (`Named`, `Typed`, `Invocation` ...).
- `entities(fineKind: "parameter", trait: "Invocation", ...)`: exact fine kind
  name, and trait by name (an unknown trait is an error listing the known
  ones). They combine with the other filters.
- A fine-kind root field is empty when the model defines no such kind.
- `--on-demand`: these fields need only the structure layer; no indexer runs.

```graphql
{
  parameters(first: 5) { totalCount nodes { name traits parent { name } } }
  records { nodes { name } }
  entities(trait: "Invocation", fineKind: "pharo.method", first: 3) { nodes { __typename name } }
}
```

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
and `Session`. The root must be in a git repository (blob ids); otherwise
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

(The export was accepted by a Moose image on jpetstore, see the session notes
in the `12-moose-export` commits; it was not re-run for this integration.)

**Import and diff.** Import a Moose JSON (e.g. VerveineJ output) and compare it
with a model built from the same sources:

```rust
use model_moose::{ImportOptions, diff, import_json};
let mut b = ModelBuilder::new();
let imported = import_json(&mut b, &bytes, &ImportOptions::default());
let moose = b.build();                     // keys `moose:<id>`, props `moose.*`
let report = diff(&moose, &model);        // rows by FM3 class, archetype, relation kind
```

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
  (files, functions, plain variables) are not exported. Class `Exception`
  promotion and `Access.variable` are workarounds learned from a Moose image.
- **Moose import**: real VerveineJ exports can be invalid strict JSON (raw
  control characters); the reader is tolerant.
- **Other languages**: TypeScript/Python/Pharo have few fine kinds;
  FM3 class names for TypeScript and Smalltalk were written from the Famix
  repositories and **not** checked against an export; Pharo trait usage,
  extensions, stubs and metaclasses are missing ([traits.md](traits.md)).
- [famix.md](famix.md) §3 is the analysis made *before* the epic: its "Gap"
  tables describe what was missing then.
