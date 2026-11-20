# On-demand import

`model_factory::build` computes everything: every language, every file, the
SCIP index, all relations. **On-demand import computes only what a request
asks for**, and remembers it. This guide explains the model of it, how to use
it from Rust and from GraphQL, what triggers what, and where it stops helping.

Reference: [design.md](design.md) section 4.1 (numbers), [usage.md](usage.md)
section 4 (quick start). Code: `crates/model-session` (library) and
`crates/model-graphql/src/ondemand.rs` (GraphQL).

## 1. The idea in one picture

A request is a pair: **a scope** (which files) and **a requirement** (which
traits, which analyses). The session turns it into the cheapest set of
layers, runs only those, and keeps the result.

```
 request ─ scope ───────► files     All | Files | Package | GitDiff
         └ requirement ─► traits ─► layers ─► work done
                                     │
   structure  (L0)  tree-sitter on the scope's files, fragments cached by blob;
                    direct frontends (Pharo). Never starts an indexer.
   resolution (L1)  SCIP index: reused, composed from the cache, or produced
                    by an indexer (policy: --reindex). Needs structure.
   derived    (L2)  named analyses over the frozen model
                    (`interface-calls`). Needs resolution.
```

Nothing is built until someone asks. A request that touches nothing builds
an empty model.

## 2. Which trait costs what

A trait goes to the resolution layer exactly when it **carries relations**
(`carries` in `crates/model-traits/traits.json`). The rest come from structure.
This follows the trait data, it is not a separate table.

| Trait | Carries | Layer |
|---|---|---|
| `Named` | | structure |
| `WithMembers` | | structure |
| `Modifiers` | | structure |
| `Annotations` | | structure |
| `WithInheritances` | `inherits`, `implements` | resolution |
| `Invocation` | `calls`, `overrides` | resolution |
| `Access` | `references`, `accesses` | resolution |
| `Typed` | `declared_type` | resolution |
| `Exceptions` | `throws` | resolution |

Dependencies are closed: asking for an analysis runs resolution, which runs
structure. Layers are data too (`LAYERS` in `crates/model-session/src/layers.rs`).

**What "resolution" gives you.** With an index, exact relations
(`Provenance::Scip`). Without one (no `index.scip` and no permission to run
the indexer), relations are resolved by name from tree-sitter
(`Provenance::TreeSitter`, best effort: see
[usage.md](usage.md) section 8). Either way, entities still come from
structure.

## 3. Scopes

| Scope | Files read |
|---|---|
| `Scope::All` | the whole root |
| `Scope::Files(vec)` | these files or directories, relative to the root |
| `Scope::Package(dir)` | one directory, e.g. `src/main/java/app/orders` |
| `Scope::GitDiff { base }` | files changed since `base` (a commit, branch or tag): committed, staged, unstaged, plus untracked |

A `GitDiff` is resolved **each time `require` is called**, not when the session
starts: new edits are seen by the next request.

Outside the scope, SCIP documents are dropped, so symbols they define become
`external` entities. A relation that points out of the scope therefore ends
on an external entity, not on a missing one. Pharo is always read whole.

## 4. From Rust

```rust
use model_session::{Requirement, Scope, Session};

let mut s = Session::new("path/to/repo");

// 1. Entities only: structure layer, no index read, no indexer.
let p = s.require(
    &Scope::Package("src/main/java/app".into()),
    &Requirement::traits(&["Named", "WithMembers"]),
)?;
assert!(p.report.indexers_started == 0);

// 2. Same scope, same traits: stored model, nothing runs.
let p = s.require(&Scope::Package("src/main/java/app".into()),
                  &Requirement::traits(&["Named"]))?;
assert!(p.report.layers_run.is_empty());

// 3. More traits on the same scope: rebuilt with the union.
let p = s.require(&Scope::Package("src/main/java/app".into()),
                  &Requirement::traits(&["Invocation"]))?;
// p.report.layers_run == [Structure, Resolution]

// 4. An analysis on top: applied to the existing model.
let p = s.require(&Scope::All,
                  &Requirement::traits(&["Invocation"]).with_analysis("interface-calls"))?;
```

`Session::with_options(root, BuildOptions)` sets the policy. Relevant fields:
`opts.scip = false` forbids the resolution layer's index;
`opts.index.reindex` is the indexer policy (`Never`, `IfMissing`, `IfStale`,
`Always`). **The library default is `IfStale`**: with `Session::new`, a request
that needs the resolution layer **can start an indexer** when the index is
missing or older than a source file. The GraphQL server and the `measure`
example set `Never` explicitly. For a path that must never pay for an indexer,
use `Session::with_options` with `Reindex::Never`.

### Caching rules

The session keeps **one entry per distinct scope**:

| New request on a known scope | What happens |
|---|---|
| same or fewer traits | the stored model is returned, `layers_run` empty |
| more traits | rebuild with the union of old and new layers (nothing is lost) |
| only missing analyses | applied to the stored model (ids unchanged), `Derived` added |
| a rebuild after analyses | analyses are applied again (a rebuild drops derived facts) |
| a different scope | its own entry, computed from scratch |

Two cheaper things sit under the session: the **fragment cache** (a file
already parsed is not parsed again, keyed by git blob) and the **SCIP document
cache** (see [design.md](design.md) section 4). A rebuild therefore costs
composition, not parsing.

`Session::clear()` forgets every computed model.

### Reading the `Report`

Every call returns `Provided { model, report }`:

| Field | Meaning |
|---|---|
| `layers_run` | layers that ran **for this call** (empty = answer was ready) |
| `layers` | layers the returned model holds |
| `scope` | resolved files, `None` = whole root |
| `files`, `fragments` | files read by tree-sitter, fragments reused vs extracted |
| `indexing_events` | the resolution layer started looking for or running an indexer |
| `indexers_started` | indexer processes **actually started** |
| `indexes` | indexes linked and how each was obtained (reused, stale, indexed, cache) |
| `analyses_run`, `warnings`, `elapsed` | as named |

Use `indexers_started == 0` to assert that a path never pays for an indexer.

## 5. From GraphQL

```bash
# the whole root, relations only on demand
cargo run --release -p model-graphql -- path/to/repo --on-demand

# one package, or several (several are treated as a file list)
cargo run --release -p model-graphql -- path/to/repo --on-demand --package src/main/java/app
# only what changed since a branch
cargo run --release -p model-graphql -- path/to/repo --on-demand --diff origin/main

# allow an indexer to run when a query needs relations
cargo run --release -p model-graphql -- path/to/repo --on-demand --reindex if-stale
```

Notes on the flags:

- `--on-demand` needs a directory (not a `model.json`) and cannot be combined
  with `--schema`. `--package` and `--diff` are exclusive.
- The **scope is fixed at startup**: one server, one scope. To look at another
  package, start another server (the session itself supports several scopes).
- The indexer policy defaults to `--reindex never`: the resolution layer
  **uses an existing `index.scip`** and never starts a process. Add
  `--reindex if-stale` (or `if-missing`, `always`) to allow it.
- Each request logs one line on stderr:
  `require: ran [Structure] of [Structure] in 38ms (46 files, 0 indexing events)`.
  `ran` is what this request paid, `of` is what the model now holds. A repeated
  query logs `ran []`.

### How a query becomes a requirement

The server walks the query's selection set (fragments followed, introspection
ignored) and maps each field to traits. The union is the requirement.

| Field(s) | Needs |
|---|---|
| `__schema`, `__type` | nothing |
| paging envelope: `nodes`, `edges`, `pageInfo`, `totalCount`, `cursor`, ranges | nothing |
| entity data: `name`, `key`, `kind`, `fineKind`, `traits`, `file`, `range`, `props`, `entity`, `entityByKey`, `entities`, `classes`, `fields`, `variables`, ... and every `...ByKey` | `Named` |
| containment: `methods`, `children`, `parent`, `ancestors` | `Named`, `WithMembers` |
| relations: `outgoing`, `incoming`, `relations`, `from`, `to`, `sites`, `siteCount`, `provenance` | traits carrying the relation kind |
| a root field the table does not know | `Named` (it is a typed concept's list, see below) |
| any **other nested** unknown field | **every trait** |

Two details on relations:

- A **literal** `kind: "calls"` narrows the requirement to the traits carrying
  `calls` (here `Invocation`). A kind no trait carries (`"imports"`, custom
  ones), a variable, or no `kind` at all, asks for every relation trait.
- Typed concepts (`parameters`, `records`...) come from the model's metamodel
  (see [usage.md](usage.md) section 4). They are entities, so they need
  structure only.

A field missing from the table can cost performance, never correctness: it
falls back to "every trait" (or structure, at the root).

### Walk-through

```graphql
# Request 1: structure only. No index read, no indexer.
{ methods(first: 20) { nodes { name parent { name } } } }
#   require: ran [Structure] of [Structure]

# Request 2: same data again.
#   require: ran [] of [Structure]               ← answered from the session

# Request 3: callers of every method, by name.
{ methods(first: 20) { nodes { name incoming(kind: "calls") { nodes { from { name } } } } } }
#   require: ran [Structure, Resolution] of [Structure, Resolution]
#   (an indexer only if --reindex allows it and the index is missing or stale)

# Request 4: typed concept, structure only again.
{ parameters(first: 5) { nodes { name traits } } }
#   require: ran [] of [Structure, Resolution]
```

The schema is rebuilt only when the model changed; a repeated request reuses
the same schema object.

## 6. When it helps, and when it does not

What it saves, in order of size:

1. **An indexer run.** Seconds to minutes when the index is missing or stale.
   A structure-only request never starts one.
2. **The SCIP load and link**, about a third of a Java build on the fixtures.
3. **Files outside the scope**, but only on large scopes (see below).

Measured on the fixtures ([design.md](design.md) section 4.1, median of 7,
existing `index.scip` only): structure on all files −32 % (Java) and −11 %
(TypeScript) against a full build; one package −49 % and −14 %. The
absolute gain is small on small projects because **a request that builds
pays about 28 ms of fixed cost** (compiling the grammar queries) plus 3–4 ms of
detection, whatever the scope. Cached answers do not pay it.

Prefer on-demand when:
- the index is absent or slow to produce and many queries do not need relations;
- you look at a package or a diff, not the whole repository;
- the process is long-lived (server, IDE), so answers are reused.

Prefer `model_factory::build` when you will read everything anyway, or for
one-shot batch work: a full build is one pass and has no per-request overhead.

## 7. Limits

- **A growing request rebuilds.** Asking for relations after structure
  recomposes the structure from fragments (parsing is skipped, composition is
  not). The structure is not kept in memory between layers.
- **Requests are serialised.** The session sits behind a lock: while a build
  runs, other requests wait.
- **One scope per GraphQL server**, fixed at startup.
- **A warm fragment cache is not a speed-up by itself**: on a very large
  repository (vscode), composing from fragments was slower than parsing
  directly. The benefit to expect is skipping layers and files.
- **One derived analysis** exists (`interface-calls`), by name.
- **Pharo is always read whole**, whatever the scope.
- **Relations leaving the scope** end on external entities.
- **Unknown nested GraphQL fields** ask for every trait: correct, not minimal.

## 8. Recipes

**Check that a query does not start an indexer.** Run the server with
`--on-demand`, send the query, read `indexing events` on the `require:` line:
it must be 0. In Rust, assert `report.indexers_started == 0`
(`crates/model-graphql/tests/ondemand.rs` does this end to end).

**Review a branch.** `--diff origin/main` and query only what you need; the
diff is resolved on every request, so new edits show up without a restart.

**Measure it on your project.**

```bash
cargo run --release -p model-session --example measure -- <root> [--package <dir>] [--runs 7]
```

It uses existing indexes only (never an indexer) and compares each layer
combination with `model_factory::build`.

**Add a derived analysis.** Implement `model_analysis::Analysis`, add its name
to `DERIVED_ANALYSES` and a branch in `derive_of`
(`crates/model-session/src/lib.rs`); then `Requirement::with_analysis(name)`.

**Make a new GraphQL field cheaper.** List it in the matching table of
`crates/model-graphql/src/ondemand.rs` (`ENTITY_FIELDS`, `MEMBER_FIELDS`,
`RELATION_FIELDS` or `ENVELOPE_FIELDS`); until then it asks for every trait.

## 9. Troubleshooting

| Symptom | Likely cause |
|---|---|
| Relations are empty or approximate | no index and `--reindex never`: the resolution layer fell back to by-name relations. Check `warnings` in the report, or allow `--reindex if-missing` |
| An indexer starts unexpectedly | the query follows relations, or a nested field the table does not know asked for every trait; read the `require:` line and the `indexing events` count |
| `--on-demand needs a directory` | you passed a `model.json`: on-demand builds from sources |
| `--diff and --package are exclusive` | pick one scope |
| `parameters` (or another typed concept) is not in the schema | the model has no entity of that kind yet in this scope; concepts exist only for kinds the model contains |
| Requests feel slow although nothing changed | something forces a rebuild: a new scope, or more traits than before; the log shows `ran [...]` non-empty |
