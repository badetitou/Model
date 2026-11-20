# Examples: a cookbook

Nine end-to-end scenarios on one small project, with the exact commands, the
output to expect and what can go wrong. Every command here was run on the
0.4.0 release branch and the output quoted is the real one (timings
aside). The scenarios that have a Rust program or a test behind them say so:
`cargo test --workspace` runs them, so this page cannot drift from the code.

New here? Read the [README](../README.md) first, then this page; the
[usage guide](usage.md) is the reference for the APIs used below.

| Scenario | Goal | Runs on |
|---|---|---|
| [a](#a-build-a-model-and-query-it-from-rust) | build a model, query it by kind, trait, relation | `tour`, `sessions` examples |
| [b](#b-serve-it-over-graphql) | typed concepts, `fineKind`/`trait` filters, new relations | `model-graphql` + test |
| [c](#c-on-demand-one-package-or-a-git-diff) | serve a package or a diff, no indexer | `--on-demand` + test |
| [d](#d-incremental-rebuild-through-the-fragment-cache) | warm rebuilds | `tour` example |
| [e](#e-export-to-moose-and-load-it) | Moose JSON export, loaded in Moose | `export` example + test |
| [f](#f-import-a-verveinej-model-and-diff-it) | import, diff, what VerveineJ shows | `import` example + test |
| [g](#g-a-project-scip-java-cannot-drive) | `--index-command` | `build` example |
| [h](#h-declare-a-new-language) | query file, `kinds.json`, concept names | `new_concept` example + test |
| [i](#i-generic-analyses) | degrees, cycles, unused, impact | `tour` example |

## The sample project

`docs/examples/shop/` is eight small Java files, written for this page:

```
docs/examples/shop/src/main/java/shop/
  api/Priced.java              interface Priced { Money price(); }
  api/Audited.java             @interface Audited { String value() default ""; }
  model/Money.java             record Money(long cents, String currency), plus()
  model/Status.java            enum Status { NEW, PAID, SHIPPED }
  model/Order.java             class Order implements Priced; List<Money>, += on a field
  service/Repository.java      interface Repository<T> { T find(String) throws NotFoundException; ... }
  service/NotFoundException.java   class ... extends Exception
  service/OrderService.java    implements Repository<Order>; @Override, @Audited,
                               try/catch, locals, a find/totalCents/retry cycle, an unused helper
```

It has **no `pom.xml` and no index**: no project root is detected, so no
indexer could ever start, whatever the options. Everything below is
tree-sitter only (entities and relations resolved *by name*). That is what you
get on any tree you have not indexed; scenario (g) and
[usage.md](usage.md) cover the indexed case.

Run from the repository root. `cargo` builds in debug mode here for brevity;
add `--release` on real projects.

## a. Build a model and query it from Rust

```sh
cargo run -p model-factory --example build -- docs/examples/shop --no-scip --no-cache
```

```
language java — 8 files, project roots []
tree-sitter java (8 files, 59 definitions, 8 unresolved): relations resolved by name
fragments: 0 reused from the cache, 8 parsed
80 entities (15 types, 16 callables, 9 external)
77 relations, 81 sites, 0.0 MiB, languages ["java"], in 195.7 ms
```

`--no-scip` is explicit but redundant here (the CLI default is
`--reindex never`, and there is no index); `--no-cache` keeps the document and
fragment caches out of `.git`. Add `--json model.json` to save the model (it
is the input of scenarios b, e and f).

The same thing from Rust, then queries. This is
[`crates/model-factory/examples/tour.rs`](../crates/model-factory/examples/tour.rs):

```sh
cargo run -p model-factory --example tour
```

```rust
let opts = BuildOptions { scip: false, cache: None, ..BuildOptions::default() };
let build = model_factory::build(&root, &opts, &|_| {})?;
let model = &build.model;

// By archetype (15 kinds) or by fine kind (a row of the metamodel).
let classes = model.of_kind(EntityKind::Class);
let id = model.metamodel().find("record").unwrap();
let records: Vec<_> = model.of_kind_id(id).map(|e| e.name()).collect();
```

```
== by kind
Namespace: api, java, model, service, shop, util
Interface: Audited, Priced, Repository
Class: ArrayList, Exception, HashMap, List, Map, Money, NotFoundException, Order, OrderService, String, T
record: Money
annotation_type: Audited
enum_constant: NEW, PAID, SHIPPED
type_parameter: T
parameters: 15, local variables: 4, annotations: 4
```

Read this output carefully, it shows what a name-based model is:

- `Money` is in `Class` **and** is a `record`: the archetype is `Class`, the
  fine kind refines it. `Audited` is an `Interface` and an `annotation_type`.
  `Status` is an `Enum`, not a `Class`.
- `ArrayList`, `List`, `Map`, `String`, `Exception`, `HashMap`, the namespaces
  `java` and `util`, and `T` (the type parameter of `Repository`) are
  **external** entities: used but not defined in the sources. Tree-sitter has
  no type resolution; it creates a stub from the name. `e.external()` tells
  them apart, and the `T` stub is a known artefact (a SCIP index resolves it
  to the type parameter).

**By trait.** Traits are carried by kinds; a trait query needs `model-traits`
for the mask. `crates/model-session/examples/sessions.rs`:

```rust
let members = model_traits::mask(&["WithMembers"]);
let with_members = model.of_trait(members).filter(|e| !e.external()).map(|e| e.name());
```

```sh
cargo run -p model-session --example sessions
```

```
all, Named+WithMembers: 71 entities, ran [Structure], 8 files, 0 indexing events, 0 indexers started
WithMembers: Audited, Money, NotFoundException, Order, OrderService, Priced, Repository, Status, api, model, service, shop
Typed kinds: constructor, field, local_variable, method, parameter
again, Named: 71 entities, ran [], 0 files, 0 indexing events, 0 indexers started
package shop/model, WithMembers: 31 entities, ran [Structure], 3 files, 0 indexing events, 0 indexers started
all, Invocation: 80 entities, ran [Structure, Resolution], 8 files, 0 indexing events, 0 indexers started
```

`WithMembers` holds the types and the namespaces; `Typed` is carried by fields,
methods, constructors, parameters and locals. Methods of every language carry
the same traits (they come from the archetype, see usage.md §8).

**Relations.** The four relations of epic #1, read from the tour:

```rust
let find = /* OrderService.find */;
find.targets(&RelationKind::Overrides);      // Repository.find
find.targets(&RelationKind::Throws);         // NotFoundException
find.targets(&RelationKind::DeclaredType);   // Order (the return type)
for r in add_line.outgoing().filter(|r| *r.kind() == RelationKind::Accesses) {
    r.site_flags();                           // READ / WRITE / RECEIVER per site
}
```

```
== relations
OrderService.find overrides model java . . shop/service/Repository#find().
OrderService.find throws NotFoundException
OrderService.find returns Order
Order.addLine accesses lines (read|receiver)
Order.addLine accesses revisions (read|write)
Order.addLine accesses line (read)
Order.addLine has a read+write access: true
```

`revisions += 1` is one site flagged `read|write`; `lines.add(line)` reads
`lines` and uses it as a receiver; `line` is a parameter.

**What can go wrong.**

- *Calls and overrides are matched by name.* `order.price()` in
  `OrderService` does not resolve: `price` exists in `Priced` and in `Order`
  and the receiver's type is not known, so the call is counted as unresolved
  (the `8 unresolved` of the build line). A unique name resolves; an ambiguous
  one does not. Overloads are not told apart for `overrides`. An index fixes
  all of this: `build ... --reindex if-stale` with a project that has a
  `pom.xml`/`build.gradle` (scip-java must be installed).
- *`kind` lookups return `Option`/iterators, nothing panics*; but
  `model.metamodel().find("parameter")` is `None` on a model whose sources
  have no parameter (a kind exists once the frontend has met it; likewise the
  schema then has no `parameters` root). The tour fails with
  ``no `record` kind in the model`` on a tree without a record.
- `Model` is immutable. To change it, rebuild (scenario d) or use
  `ModelBuilder::from_model`.

## b. Serve it over GraphQL

```sh
cargo run -p model-graphql -- docs/examples/shop --no-scip --no-cache
# GraphiQL on http://127.0.0.1:8000/ , API on /graphql
```

`--port 8765` picks another port. Then in GraphiQL (or with `curl`), the typed
concepts the model's metamodel names:

```graphql
{ parameters(first: 3) { totalCount nodes { name traits parent { name } } }
  records { nodes { name fineKind } } }
```

```json
{"data":{"parameters":{"totalCount":15,"nodes":[
  {"name":"cents","traits":["Named","Modifiers","Annotations","Typed"],"parent":{"name":"Money"}},
  {"name":"currency","traits":["Named","Modifiers","Annotations","Typed"],"parent":{"name":"Money"}},
  {"name":"other","traits":["Named","Modifiers","Annotations","Typed"],"parent":{"name":"plus"}}]},
 "records":{"nodes":[{"name":"Money","fineKind":"record"}]}}}
```

Other roots present on this model: `annotationTypes` (`Audited`),
`enumConstants` (3), `localVariables`, `typeParameters`, `annotations`,
`comments`. A concept root exists only when the model has the kind: this
sample has no TypeScript, so `tsInterfaces` is absent from its schema (print it
with `--schema docs/examples/shop`; `--schema` alone prints the bundled-data
schema, `crates/model-graphql/schema.graphql`).

**`fineKind` and `trait` filters** work for every kind, with or without a
concept:

```graphql
{ entities(fineKind: "parameter", trait: "Typed", first: 2) {
    totalCount nodes { __typename name } } }
```
```json
{"data":{"entities":{"totalCount":15,"nodes":[{"__typename":"Parameter","name":"cents"},{"__typename":"Parameter","name":"currency"}]}}}
```

**The new relations**, with flags on each occurrence:

```graphql
{ entities(name: "addLine") { nodes { outgoing(kind: "accesses") { nodes {
    to { name fineKind }
    flaggedSites { nodes { flags read write } } } } } } }
```
```json
[{"to":{"name":"lines","fineKind":"field"},"flaggedSites":{"nodes":[{"flags":["read","receiver"],"read":true,"write":false}]}},
 {"to":{"name":"revisions","fineKind":"field"},"flaggedSites":{"nodes":[{"flags":["read","write"],"read":true,"write":true}]}},
 {"to":{"name":"line","fineKind":"parameter"},"flaggedSites":{"nodes":[{"flags":["read"],"read":true,"write":false}]}}]
```

```graphql
{ entities(name: "find", kind: METHOD) { nodes { parent { name }
    overrides: outgoing(kind: "overrides") { nodes { to { key } provenance } }
    throws: outgoing(kind: "throws") { nodes { to { name } } }
    returns: outgoing(kind: "declared_type") { nodes { to { name } } } } } }
```

returns two methods: `OrderService.find` (overrides
`model java . . shop/service/Repository#find().`, provenance `TREE_SITTER`,
throws `NotFoundException`, returns `Order`) and `Repository.find` (throws
`NotFoundException`, returns `T`).

[`crates/model-graphql/tests/examples.rs`](../crates/model-graphql/tests/examples.rs)
runs these queries on the sample and asserts the answers.

**What can go wrong.**

- `entities(trait: "Nope")` is an error listing the known traits
  (`Named, WithMembers, WithInheritances, Invocation, Access, Modifiers,
  Annotations, Typed, Exceptions`). `entities(fineKind: "nope")` is **not** an
  error, it is simply empty: check the spelling in `Entity.fineKind`.
- `__typename` of a record is `Record`, not `Class`: `classes { ... }` and
  `... on Class` hold only the class-archetype entities **without** a concept
  of their own (here `Order`, `NotFoundException`, the externals).
  `entities(kind: CLASS)` matches all of them, records included.
- `methods(first:)` lists only entities typed `Method`; use
  `children(kind: METHOD)` for every kind of method.
- Queries are limited to depth 16 (`MAX_DEPTH`).
- Browser clients on another origin need `--allow-origin http://localhost:5173`.

## c. On-demand: one package or a git diff

`--on-demand` builds nothing at startup; each query's selection set decides
which layers run, on the files of the scope. Full reference:
[on-demand.md](on-demand.md).

```sh
cargo run -p model-graphql -- docs/examples/shop --on-demand --package src/main/java/shop/model
```

Query `{ classes { nodes { name } } }`, then `{ parameters { totalCount } }`,
then `{ entities(name:"price", kind: METHOD) { nodes { outgoing(kind:"calls") { totalCount } } } }`.
The server logs one line per request on stderr; this is the real log:

```
GraphiQL on http://127.0.0.1:8767/
require: ran [Structure] of [Structure] in 227.290083ms (3 files, 0 indexing events)
require: ran [] of [Structure] in 11.5µs (0 files, 0 indexing events)
require: ran [Structure, Resolution] of [Structure, Resolution] in 223.721542ms (3 files, 0 indexing events)
```

Read it as: `ran` is what this request paid, `of` is what the model holds now,
`(N files, M indexing events)` the files read and the number of times the
indexing step started looking for an index.

1. `classes` (answer: only `Order`, the one class of the package): structure
   layer, **3 files**, not 8.
2. `parameters`: a typed root needs only the structure: `ran []`, answered
   from the session.
3. Following `outgoing(kind:"calls")` adds the resolution layer. **0 indexing
   events**: nothing looked for an index, nothing was started. On a project
   with a build marker the line would say `1 indexing events` and, with
   `--reindex never` (the default of the server), the layer still only *uses*
   an existing `index.scip`; it runs an indexer only if you pass
   `--reindex if-stale|if-missing|always`.

**Checking that no indexer starts** is exactly this: `0 indexing events` on the
`require:` line (`Report::indexing_events`; `Report::indexers_started` counts
processes actually spawned). In Rust,
`model-session/examples/sessions.rs` prints both.

**A git diff.** The scope is the files changed since a revision (committed,
staged, unstaged, plus untracked). On a throwaway copy of the sample:

```sh
cp -R docs/examples/shop /tmp/shop-demo && cd /tmp/shop-demo
git init -q && git add -A && git commit -qm init
echo '// touched' >> src/main/java/shop/model/Order.java
cd - && cargo run -p model-graphql -- /tmp/shop-demo --on-demand --diff HEAD
```

`{ methods { totalCount nodes { name } } }` answers the five methods of
`Order.java` only (`id`, `addLine`, `pay`, `status`, `price`), and the log is
`require: ran [Structure] of [Structure] in ... (1 files, 0 indexing events)`.
Asking for `outgoing(kind: "accesses")` then logs
`ran [Structure, Resolution]`, again with 0 indexing events.

**What can go wrong.**

- The scope is fixed when the server starts (the files of a diff are read
  again by each request, but the base is fixed): restart to look at another
  package. `--diff` and `--package` are exclusive; `--on-demand` needs a
  directory, not a `model.json`, and cannot be combined with `--schema`.
- An empty diff is a scope of **no files**: you get an empty model, not the
  whole tree (checked: `methods { totalCount }` is 0, `0 files` on the log). A base that is not a revision is an error from `git`.
- Fixed cost: every request compiles the grammar queries (about 28 ms,
  on-demand.md §6), so on a small project on-demand gains little; it pays on
  big trees.
- An unknown nested field makes the server ask for every trait (so the
  resolution layer): if an indexer starts unexpectedly, read the `require:`
  line.
- The doc's `in 38ms` is illustrative; the real line prints a Rust `Duration`
  (`227.290083ms`, `11.5µs`).

## d. Incremental rebuild through the fragment cache

Each source file yields a serialisable `Fragment`, cached under the git blob
of its content. A rebuild reparses only the files that changed.

```sh
cargo run -p model-factory --example build -- docs/examples/shop --no-scip --fragment-cache /tmp/fc
```

Run it twice, then edit a file and run it a third time (the lines `fragments:`):

```
fragments: 0 reused from the cache, 8 parsed        # cold
fragments: 8 reused from the cache, 0 parsed        # nothing changed
fragments: 7 reused from the cache, 1 parsed        # after appending a comment to Status.java
```

From Rust, `tour` does it with `extract_fragments` and a `FragmentCache`:

```rust
let cache = FragmentCache::at(dir);
let (set, stats, warning) = extract_fragments(&root, &ExtractOptions::default(), Some(&cache))?;
let mut b = ModelBuilder::new();
set.compose(&mut b, true);          // true = relations resolved by name
```
```
== fragments
cold: 8 parsed, 0 reused, 80 entities
warm: 0 parsed, 8 reused, 80 entities
```

In `BuildOptions`, `fragment_cache_dir` picks the directory; by default (when
`cache` is `Some("model")`, the default) it is
`<git-common-dir>/model/fragments`, shared by worktrees.

**What can go wrong.**

- The default location needs a git repository: outside one there is no cache,
  silently. An explicit directory (`--fragment-cache`, `FragmentCache::at`)
  works anywhere, checked on a copy of the sample outside git. Blob ids are
  hashed from the files on disk, so uncommitted edits are reused or reparsed
  correctly.
- A "warm" rebuild still composes the whole model: on very large trees (vscode,
  219 MB of fragments) it can be slower than a direct parse (usage.md §8,
  design §4.1).
- The cache key includes the extractor, its version and its query, so
  upgrading Model does not reuse stale fragments.

## e. Export to Moose and load it

```sh
cargo run -q -p model-factory --example build -- docs/examples/shop --no-scip --no-cache --json /tmp/shop.json
cargo run -q -p model-moose --example export -- /tmp/shop.json /tmp/shop-moose.json --root docs/examples/shop
```

`--root` is the directory the model's relative file names resolve against: the
export reads the sources to write character-offset anchors. Without it (or
with `--no-sources`) no anchors are written. The report:

```
253 objects: 72 entities, 76 associations, 105 anchors (0 skipped, 0 unreadable files), 1 stubs, 2 implicit variables, 0 generated packages
entities:
         4  AnnotationInstance          2  AnnotationType     5  Attribute
        10  Class    1  Enum    3  EnumValue    1  Exception    2  ImplicitVariable
         1  Interface   4  LocalVariable   16  Method   6  Package
        15  Parameter   1  ParametricInterface   1  TypeParameter
associations:
        31  Access    27  EntityTyping    2  Implementation    7  Import
         1  Inheritance    5  Invocation    3  Reference
entities not exported (kind):
         3  comment
         8  file
relations not exported:
         1  imports between entities that cannot import
         3  overrides (derived in Famix)
         2  references outside a method or to a non-type (Famix: method to type)
```

(One entity per line in the real output.) In a Pharo image with Moose
installed:

```smalltalk
model := FamixJavaModel new.
'/tmp/shop-moose.json' asFileReference readStreamDo: [ :s |
    model importFromJSONStream: s ].
{ model size. (model allWithType: FamixJavaClass) size.
  (model allWithType: FamixJavaMethod) size.
  (model allWithType: FamixJavaInvocation) size.
  (model allWithType: FamixJavaAccess) size }
"-> #(253 10 16 5 31)"
```

That is what I ran in a Moose image for this page: the JSON loaded without
error and the counts equal the export report. The model is not registered in
the image.

**What the export leaves out** (all counted in the report; usage.md §8 has
the long list):

- Kinds with no FM3 class: files and comments (here 8 and 3), plain
  functions, plain variables.
- `overrides` (a derived property in Famix), imports between entities Famix
  does not let import, references that are not "method to type".
- The receiver of a call is not kept: only `self` receivers are generated.
  Derived properties, metrics, `previous`/`next`, signatures (unless the
  model was imported from Moose), generic arguments (a generic type such as
  `Repository<T>` becomes a `ParametricInterface`, but there is no
  `Concretization`) are not written.

**What can go wrong.** A tree-sitter model with externals gets stubs
(`1 stubs`: the annotation `Override`); entities whose kind has no FM3 class
are listed, not silently dropped; a Model JSON from `--json` is required as
input (not a source directory). The Moose-side rules (`Exception` promotion,
`Access.variable`) are workarounds learned from a Moose image, see the
`model-moose` module doc.

[`crates/model-moose/tests/examples.rs`](../crates/model-moose/tests/examples.rs)
asserts the export report on the sample.

## f. Import a VerveineJ model and diff it

```sh
cargo run -q -p model-moose --example import -- verveine.json --root path/to/sources --diff model.json
```

`verveine.json` is a Moose JSON, e.g. VerveineJ's (`-format json`), and
`model.json` a Model JSON of the same sources (`build ... --json`). The import
prints a report, then the diff: for each FM3 class, archetype and relation
kind, `left` (the import), `right` (your model), `matched`, and a few
identities only on one side (`<` left only, `>` right only).

I could not run VerveineJ here (it needs a Docker image), so this page uses
the **export of scenario e** as a stand-in for a Moose file: a round trip, so
what it shows is the shape of the report and the loss of the export:

```sh
cargo run -q -p model-moose --example import -- /tmp/shop-moose.json --root docs/examples/shop --diff /tmp/shop.json
```

```
253 objects, 68 entities, 78 relations, 105 anchors (105 with a range, 4 unresolved)
...
entities by archetype (left, right, matched):
        12       11       11  class
...
relations by kind (left, right, matched):
        24       24       22  accesses
             < shop.model.Money.plus -> cents
             > shop.model.Money.plus -> shop.model.Money.cents
         5        5        5  calls
        27       27       26  declared_type
         2        2        2  implements
         7        8        0  imports
         1        1        1  inherits
         0        3        0  overrides
         3        5        3  references
         2        2        2  throws
```

How to read it: `calls 5/5/5` is a perfect round trip; `overrides 0/3/0` is
the relation Famix derives and the export omits; `imports 7/8/0` are
identified differently (entity vs file), the same import is not matched.
The "by FM3 class" table is split on the Model side (a `record` or a
`parameter` is a fine kind, not an FM3 class), so use the **archetype** and
**relation** tables to compare. In Rust:

```rust
let mut b = ModelBuilder::new();
let imported = import_json(&mut b, &bytes, &ImportOptions::default());
let moose = b.build();
let report = diff(&moose, &model);     // report.classes, .archetypes, .relations
```

[`docs/verveinej-comparison.md`](verveinej-comparison.md) is the real
comparison, made on VerveineJ's own sources (232 files) with and without
dependencies, on the branch state of 2026-11-20. In short:

- **Same entities, mostly.** Every internal inheritance and implementation
  pair matches (178 and 124); annotation instances (2065) and enum values
  match; real types and methods keys are identical (3352 of 3617 methods
  match exactly).
- **Relations by name are the weak spot of tree-sitter.** Of VerveineJ's
  internal calls, 74.4 % are found by tree-sitter and 99.8 % with a SCIP
  index; accesses 48.3 % (tree-sitter), reference pairs 11 of 906.
- **Largest gaps in Model, as measured then**: the generic layer (parametric
  types, concretisation), method signatures/visibility/modifiers, anonymous
  classes and initialisers, catch variables (Model: parameters), annotation
  attribute values, and library stubs without a package in tree-sitter models.
- **Gaps on the VerveineJ side**: no `Import` objects; guessed stubs when it
  runs without the dependency jars (inflates its class count from 402 to
  1138).

Several of the gaps listed there were addressed later (modifiers, fine kinds,
`accesses`, `declared_type`...): re-measure before quoting the numbers.

**What can go wrong.** Real VerveineJ output can be invalid strict JSON (raw
control characters): the reader is tolerant. File names are compared as
written (`./relative` vs absolute): pass `--root` for anchors, and expect
`fileName` mismatches. Parameters, locals and type parameters only exist on one
side of the diff when the other has no entity for them.

## g. A project `scip-java index` cannot drive

`scip-java index` only drives Gradle, Maven and Bazel. For a `make`-built
project, or any pipeline of yours, you say how the index is produced. Three
equivalent ways, for a language (`LANGUAGE=COMMAND`, the language is required):

```sh
# flag (repeatable, one per language)
cargo run -p model-graphql -- path/to/repo --reindex if-missing \
  --index-command 'java=./tools/make-index.sh {root} {output}'

# environment variable (read by the build example and model-graphql)
MODEL_INDEX_COMMAND='java=make -C {root} scip' \
  cargo run -p model-factory --example build -- path/to/repo
```

```rust
let mut opts = BuildOptions::default();
opts.index.set_custom(CustomIndex::new("java", "./tools/make-index.sh {root} {output}"));
```

`{root}` and `{output}` (the file name the index must have) are substituted
shell-quoted: write them bare. The flag replaces the environment variable for
the same language.

**What I could check without a real indexer**, on the sample, with failing
commands (the success path, a command that really writes a valid `index.scip`,
is covered by `model-index`'s tests but I did not run it end to end):

```sh
build docs/examples/shop --no-cache --reindex if-missing --index-command 'java=echo failing >&2; exit 3'
warning: docs/examples/shop/: the custom index command failed, using tree-sitter only: failing (exit status: 3); custom index command `echo failing >&2; exit 3` in docs/examples/shop/ (exit 127 = a program was not found; MODEL_INDEXER_TIMEOUT raises the time limit; run it there by hand to see the failure)

build docs/examples/shop --no-cache --reindex if-missing --index-command 'java=true'
warning: docs/examples/shop/: the custom index command succeeded but wrote no index.scip; in the project root; using tree-sitter only

build docs/examples/shop --no-cache --index-command 'oops'
Error: index command "oops": expected LANGUAGE=COMMAND, e.g. java="./make-index.sh {root} {output}"
```

A failing command is a **warning, not an error**: the language falls back to
tree-sitter (the model has 80 entities as before). It also shows that the
analysed root becomes the project root for that language even though the
sample has no build marker.

**The safety rule.** The command comes from the flag, `MODEL_INDEX_COMMAND` or
`IndexOptions::custom`, *never from a file of the analysed repository*: a
repository must not be able to run commands on whoever analyses it. Do not
forward untrusted text (a request parameter, a repository's file) into it. It
runs through `sh -c` in the project root with a timeout
(`MODEL_INDEXER_TIMEOUT`, default 600 s). With `--reindex never` an existing
`{output}` is used and nothing runs.

**Safer alternatives** when you do not want to run anything:

```sh
# tree-sitter only: entities and name-resolved relations
cargo run -p model-graphql -- path/to/repo --no-scip
# model only a part of the tree, on demand
cargo run -p model-graphql -- path/to/repo --on-demand --package src/main/java/app
```

Build the index once yourself and reuse it with `--reindex never`. A recipe for a
`make`-built Java project with the scip-java javac plugin is in
[usage.md](usage.md) (it is marked **untested** there, and still is).

## h. Declare a new language

A language is data plus, sometimes, a little code. What appears in the schema
is a typed concept per kind that names one.

**With a grammar (tree-sitter).** Four places; I verified the third and fourth
by adding a throw-away `java.service` kind (reverted) and reading the schema:

1. *Grammar and query.* A grammar crate behind a Cargo feature, a query file
   (`crates/model-treesitter/queries/<lang>.scm`; the grammar's own `tags.scm`
   is a working baseline) and a row in `src/grammars.rs` (extensions,
   namespaces, constructor name, optional `file_kind`). The capture
   vocabulary is in the crate's README. The language and its markers are a row
   of `crates/model-detect/src/languages.rs`.
2. *Which kinds.* A row in `crates/model-treesitter/kinds.json` per fine kind,
   with traits and GraphQL names, and a capture `@definition.<kind name>` in
   the query:

   ```json
   {"name": "java.service", "archetype": "class",
    "graphql_type": "Service", "graphql_root": "services",
    "traits": ["Named", "WithMembers"]}
   ```
   ```scheme
   (class_declaration name: (identifier) @name (#match? @name "Service$")) @definition.java.service
   ```
   (the experiment also excluded `Service$` names from the plain
   `@definition.class` pattern, so one class is one entity.)
3. *Rebuild.* `kinds.json` and the queries are compiled in
   (`include_str!`): no Rust to write, but the crate is rebuilt.
4. *Result.* `model-graphql docs/examples/shop --schema` then holds
   `type Service implements Entity`, `services(first: Int, after: String):
   ServiceConnection!` and `serviceByKey(key: String!): Service`, and
   `{ services { nodes { name fineKind traits } } }` answers
   `OrderService`, `java.service`, `["Named","WithMembers"]`; `OrderService`
   is no longer in `classes`.

Traits come from `model-traits` names (unknown names are ignored); a kind with
no `graphql_*` fields keeps its archetype type. Names that are not valid
GraphQL names, or clash with an archetype, a `Query` field or an earlier kind,
are dropped silently: the kind keeps its archetype type. See
[traits.md](traits.md) for what each language's kinds carry.

**Without a grammar (a Rust frontend, like `model-lang-pharo`).**
[`crates/model-graphql/examples/new_concept.rs`](../crates/model-graphql/examples/new_concept.rs):

```rust
let widget = b.define_kind("toy.widget", EntityKind::Class,
                           model_traits::mask(&["Named", "WithMembers"]), None);
b.set_kind_concept(widget, Some("Widget"), Some("widgets"));
b.entity_of_kind("model toy . . ui/Button#", widget, "Button");
```

```sh
cargo run -p model-graphql --example new_concept
```
```
widgetByKey(key: String!): Widget
widgets(first: Int, after: String): WidgetConnection!
type Widget implements Entity {
{"classes":{"nodes":[{"name":"Plain"}]},"widgets":{"nodes":[{"__typename":"Widget","fineKind":"toy.widget","name":"Button","traits":["Named","WithMembers"]}]}}
```

`set_kind_concept` returns `false` for an archetype's default kind (it
already has its archetype type). The concept names are saved in the model's
JSON, so a saved model keeps its schema. A Moose import derives them from the
FM3 class (`Famix-Java-Entities.Record` → `JavaRecord`, `javaRecords`).

**What can go wrong.** The FM3 class names for TypeScript and Smalltalk were
written from the Famix repositories and never checked against an export
(traits.md). A new language also needs a row in `model-detect` to be
detected, and a `LanguageSpec::scip_indexer` if it has an indexer.

## i. Generic analyses

`model-analysis` (re-exported as `model_factory::analysis`) is read-only:
`Analysis::run(&model)`. The `tour` example runs four on the sample:

```rust
let degrees = run(&Degree::default(), model)?;          // fan-in + fan-out
let cycles  = run(&Cycle::default(), model)?;            // strongly connected components
let unused  = run(&Unused::default(), model)?;           // nothing in or out
let impact  = run(&Propagation { seeds, options }, model)?; // distance from seeds
```

```
== analyses
degree 10: model java . . String#
degree 9: model java . . shop/model/Money#
degree 8: model java . . shop/service/OrderService#find().
Money.plus: 1 caller(s)
cycle: totalCents <-> retry
unused callables: addLine, find, pay, price, save, status, unusedHelper
impact of Money.plus: [(0, "plus"), (1, "price")]
```

- *Degrees* count every relation kind by default; `DegreeOptions { kinds,
  provenance, weighted }` restricts them (here `Calls` only for the
  `Money.plus` callers; `weighted` counts sites).
- *Cycles* default to `Calls` and `References`: `totalCents` calls `retry`,
  which calls `totalCents`.
- *Unused* means no `calls`, `references` or `implements` relation in or
  out, so it lists *candidates*: `find`, `price`, `save` and `pay` are public
  API of the sample, only unreferenced inside it (and `find` and `price` are
  also hidden by the name ambiguity of scenario a). `unusedHelper` is the one
  that is truly dead.
- *Impact*: `Direction::Callers` from `Money.plus` reaches `price` at
  distance 1. `seeds_where(model, predicate)` builds seeds from any fact.

**What can go wrong.** The analyses are as good as the relations: with
tree-sitter relations (name-based) an ambiguous call is absent, so callers are
under-counted and "unused" over-reported; an SCIP index makes them reliable.
Results are indexed by `EntityId`; ids are local to a model, rebuild means new
ids (compare by key). `InterfaceCalls` (a derived analysis) returns relations
to add with `DerivedRelations::apply`, or is requested through a session with
`Requirement::with_analysis("interface-calls")`.

[`crates/model-factory/tests/tour.rs`](../crates/model-factory/tests/tour.rs)
runs the tour and asserts these lines.

## What I could not run

- A real VerveineJ export (scenario f): the stand-in is Model's own export;
  the comparison document is a measurement from another session.
- A successful `--index-command` run that produces a real `index.scip`
  (scenario g); and any scip-java run.
- The jpetstore/vscode figures in the README (they need Structure's fixtures,
  which this page does not use).
