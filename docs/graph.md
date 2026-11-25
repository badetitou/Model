# Model as a graph engine

Model already stores a codebase as entities and relations. This page documents
the deterministic graph layer built on that model: a projection format
(`model.graph/1`), a handful of graph analyses, and the `model graph` commands
that expose them. It is the same model, viewed as a graph — not a second model
and not a new index.

The analyses live in `model-analysis` (read-only over a frozen `Model`); the
projection, the Markdown report and the CLI surface live in `model-cli`.
External tools consume the projection as data; this repository generates no
visualization and claims no compatibility with any other graph format.

## Pipeline

```
sources ──► model build ──► model.json ──► model graph … ──► model.graph/1 JSON
                                 │              │                    │
                                 ▼              ▼                    ▼
                     frozen Model (loaded   Markdown report    external consumers
                     once): model-analysis   (graph report)     (Fluid, LLM-hosted
                     path · explain ·                            tools, scripts)
                     communities · degrees
```

- `model build` is still how `model.json` is produced. The graph commands take
  that saved model (or, through the library, an already-built frozen `Model`)
  and never re-parse sources or start an indexer.
- `model-session` and the MCP server keep a model in memory for repeated
  queries; the CLI is stateless and loads `model.json` on every invocation.
  The MCP tools `model_path`, `model_explain`, `model_communities` and
  `model_god_nodes` expose the same in-memory analyses; `model_export_graph`
  writes the projection and report to a directory ([mcp.md](mcp.md)).
- Analyses do not mutate the model. A projection is derived data that can be
  written to a file, piped to another tool, or discarded.

## Graph analyses

All of these are deterministic and read-only:

- **`path`** — a breadth-first shortest chain of relations between two exact
  keys. Neighbours are visited in `(target id, kind, provenance, relation id)`
  order, so ties never depend on relation insertion order. Direction is
  `callers`, `callees` or `both` (the default); walking backwards swaps the hop
  endpoints but keeps the original relation's site. The default relation kinds
  are `calls` and `references`; provenance is unrestricted; `--depth 0`, like an
  omitted `--depth`, means no cap. An unreachable pair yields empty `nodes` and
  `hops`.
- **`explain`** (neighborhood) — the immediate surroundings of one entity.
  Neighbours are aggregated by other entity, side (`incoming` / `outgoing`),
  relation kind and provenance, sorted by decreasing `weight` and then by id,
  side, kind and provenance. `in_degree` and `out_degree` count all matching
  relations, independently of `--limit` (`0` or omitted means no cap).
- **`communities`** — a deterministic Louvain-like partition of the undirected,
  site-weighted relation graph (details below).
- **God nodes** — the highest-degree entities, taken from the existing
  `Degrees::ranked()` analysis. There is no new scoring or classification step;
  `graph report` and the MCP tool `model_god_nodes` rank with it.

Filtering is always over **relations**: `--kinds` names relation kinds and
`--provenance` names one provenance. Omitting `--kinds` means no kind filter for
`projection` and `explain`; `path` defaults to calls and references. Omitting
`--provenance` means all provenances (the CLI has no provenance filter on
`explain` or `communities`). Passing an empty `--kinds` list selects no kinds
for these commands (it is not a way to say "all"), unlike `impact` and
`analysis`, where an empty list means the analysis default.

## The `model.graph/1` projection

`model.graph/1` is Model's own projection format: a JSON document with five
top-level sections.

| Section | Contents |
|---|---|
| `schema` | Format identifier, `model.graph/1`. |
| `summary` | Producer, counts, modularity and the options actually used. |
| `communities` | Community records, in ascending smallest-member order. |
| `nodes` | One record per entity in the model, including externals, in entity-id order. |
| `edges` | One record per relation retained by the filters, sorted by (from, to, kind, provenance). |

### Summary fields

| Field | Meaning |
|---|---|
| `producer` | Fixed string `model 0.8.0`; no timestamp. |
| `entities` | Number of emitted nodes. |
| `relations` | Number of emitted edges. |
| `sites` | Number of **emitted** sites (`0` with `--no-sites`), not the model's raw site count. |
| `communities` | Number of emitted communities (`0` with `--no-communities`). |
| `modularity` | Final modularity of the partition; `0.0` with `--no-communities`, and possibly negative at resolution 1. |
| `options` | The filters used: `kinds` and `provenance` (`null` when absent), `with_communities`, `with_props`, `with_sites`. |

### Node fields

| Field | Meaning |
|---|---|
| `id` | Stable entity key (the same key as in `model.json`); node identity. |
| `name` | Simple entity name. |
| `kind` | Archetype (`class`, `method`, ...). |
| `fine_kind` | Fine kind from the metamodel (`record`, `parameter`, ...). |
| `language` | Language id, or `null`. |
| `file` | Source file path, or `null` for an entity without a file. |
| `line` | 1-based line, or `null` without a range; `model-core` ranges are 0-based. |
| `parent` | Key of the containing entity, or `null`. |
| `external` | True when the entity is referenced but not defined in the sources. |
| `degree_in`, `degree_out` | Incident relations; occurrence sites do not add degrees. |
| `community`, `community_label` | Numeric community id and its label. Both default to `0` and `""` with `--no-communities`. |
| `traits` | Sorted trait names decoded from the entity's fine kind via `model-traits`; empty when the kind carries none. |
| `props` | The entity's open property map (`{}` with `--no-props`). |

### Edge fields

| Field | Meaning |
|---|---|
| `from`, `to` | Node ids (stable entity keys); both endpoints are always nodes. |
| `kind` | Relation kind (`calls`, `references`, ...). |
| `provenance` | Lowercase provenance: `tree_sitter`, `scip`, `frontend`, `derived`. |
| `weight` | Number of occurrence sites, or `1` for a site-less relation. |
| `sites` | Emitted occurrences with `file`, 1-based `line` and `flags` (`read`, `write`, `receiver`); empty with `--no-sites`. |

### Community fields

| Field | Meaning |
|---|---|
| `id` | Numeric (`usize`) community id, also used by node `community`. |
| `label` | `dominant-file:highest-degree-member`, with a `#n` suffix on collision. |
| `size` | Number of member nodes. |
| `members` | Node ids (stable entity keys), in entity-id order. |
| `internal_weight` | Sum of internal edge weights, counted once (not twice per direction). |
| `degree` | Sum of the degrees of the members in the undirected graph. |

### How communities are computed

The relation graph used for communities is undirected and site-weighted:

- only relations passing the same kind and provenance filters are kept;
- each relation contributes `max(number of sites, 1)`;
- both directions between a pair of entities are summed into one edge;
- self-loops are skipped, and isolated entities remain singletons.

Detection is Louvain-like and deterministic: single nodes move to a neighbouring
community only while modularity improves (gain epsilon `1e-9`, ties to the
smallest community id), then communities are collapsed into a quotient graph and
the process repeats. Every choice is made in numeric order, so the same model
gives the same partition and the same bytes.

Labels take the most frequent member file (ties by file name, `<no file>` when
no member has a file) and the name of the highest-degree member (ties by smallest
entity id). A repeated base label gets `#2`, `#3`, ... appended. At resolution
`1.0` the modularity of a partition can be negative; it is not a percentage.

With `--no-communities`, the projection emits an empty `communities` list,
`modularity: 0.0`, and every node gets `community: 0` with an empty label.

### Example

Shape only: names, keys, lines and counts below are illustrative, not the
output of a command. All five top-level sections and all summary fields are
present.

```json
{
  "schema": "model.graph/1",
  "summary": {
    "producer": "model 0.8.0",
    "entities": 3,
    "relations": 1,
    "sites": 2,
    "communities": 1,
    "modularity": 0.75,
    "options": {
      "kinds": null,
      "provenance": null,
      "with_communities": true,
      "with_props": true,
      "with_sites": true
    }
  },
  "communities": [
    {
      "id": 0,
      "label": "src/main/java/shop/model/Money.java:plus",
      "size": 3,
      "members": [
        "model java . . shop/model/Money#",
        "model java . . shop/model/Money#plus().",
        "model java . . java/lang/String#"
      ],
      "internal_weight": 2,
      "degree": 2
    }
  ],
  "nodes": [
    {
      "id": "model java . . shop/model/Money#",
      "name": "Money",
      "kind": "class",
      "fine_kind": "record",
      "language": "java",
      "file": "src/main/java/shop/model/Money.java",
      "line": 3,
      "parent": null,
      "external": false,
      "degree_in": 0,
      "degree_out": 0,
      "community": 0,
      "community_label": "src/main/java/shop/model/Money.java:plus",
      "traits": ["Named", "WithMembers"],
      "props": {}
    },
    {
      "id": "model java . . shop/model/Money#plus().",
      "name": "plus",
      "kind": "method",
      "fine_kind": "method",
      "language": "java",
      "file": "src/main/java/shop/model/Money.java",
      "line": 12,
      "parent": "model java . . shop/model/Money#",
      "external": false,
      "degree_in": 0,
      "degree_out": 1,
      "community": 0,
      "community_label": "src/main/java/shop/model/Money.java:plus",
      "traits": ["Named", "Invocation", "Typed"],
      "props": {}
    },
    {
      "id": "model java . . java/lang/String#",
      "name": "String",
      "kind": "class",
      "fine_kind": "class",
      "language": "java",
      "file": null,
      "line": null,
      "parent": null,
      "external": true,
      "degree_in": 1,
      "degree_out": 0,
      "community": 0,
      "community_label": "src/main/java/shop/model/Money.java:plus",
      "traits": ["Named"],
      "props": {}
    }
  ],
  "edges": [
    {
      "from": "model java . . shop/model/Money#plus().",
      "to": "model java . . java/lang/String#",
      "kind": "calls",
      "provenance": "tree_sitter",
      "weight": 2,
      "sites": [
        {
          "file": "src/main/java/shop/model/Money.java",
          "line": 13,
          "flags": ["read"]
        },
        {
          "file": "src/main/java/shop/model/Money.java",
          "line": 14,
          "flags": ["read"]
        }
      ]
    }
  ]
}
```

## Invariants

- **Stable identity.** A node's `id` is the entity's stable key, the same one
  used by `model.json`. Entity ids local to a model are never exposed here.
- **Deterministic order and bytes.** The same model and the same options give
  the same order and byte-identical output across runs. There are no
  timestamps, no random seeds and no absolute paths of the working directory.
- **1-based lines.** `line` is 1-based for readers; the underlying
  `model-core` `Range` is 0-based. Do not mix the two.
- **Closed edges.** Every edge endpoint is a node in the same projection,
  including external entities. Externals are nodes, never dangling ids.
- **No filters change identity.** Applying `--kinds`, `--provenance`,
  `--no-props` or `--no-sites` changes what is emitted, not the keys of what
  remains. The degree counts describe the emitted edges.

## Commands

`model graph <command> <model.json> [options]`; with a source checkout use
`cargo run -q -p model-cli -- graph …`. `projection`, `path`, `explain` and
`communities` print deterministic JSON (to stdout; `projection` also accepts
`--out <file|->`); **`report` prints Markdown** (to stdout or `--out`).
See [cli.md](cli.md) for the full option list.

```sh
# Full projection, pretty-printed, to a file ('--out -' means stdout).
model graph projection model.json --out graph.json --pretty

# Restrict to resolved relations and drop properties and occurrence sites.
model graph projection model.json --out graph-calls.json \
  --kinds calls,references --provenance scip --no-props --no-sites

# Markdown report: summary, communities, top god nodes, cross-community edges.
model graph report model.json --top 20 --out report.md

# Path between two entities; keys come from `model query model.json --key …`.
model graph path model.json --from '<key A>' --to '<key B>' \
  --direction both --depth 5 --kinds calls,references

# Neighborhood of one entity.
model graph explain model.json --key '<key>' --limit 20 --pretty

# Communities only; --top 0 (the default) means no cap.
model graph communities model.json --top 10 --pretty
```

`--depth 0` (`path`), `--limit 0` (`explain`) and `--top 0` (`report` and
`communities`) all mean "no cap". `graph communities --top N` truncates the
emitted array but keeps `total` as the full detected count. `graph report`
always builds its projection with the default options (all relation kinds and
provenances, communities/properties/sites included). The CLI exposes no
kind or provenance filters on `graph communities`.

## Limits and non-goals

This layer projects what the model already knows. It deliberately does **not**:

- generate or ship a visualization. External tools (for example Fluid or
  LLM-hosted tools) consume `model.graph/1` as an input format;
- depend on any external graph package, format or service;
- add an ambiguity or confidence layer for an LLM to resolve — there is no
  "maybe" edge and no confidence score;
- extract documentation, PDFs or semantic knowledge beyond the code model;
- infer dynamic dispatch (interface calls remain an explicit, optional
  analysis, not an implicit graph edge);
- watch files or install hooks — rebuild the model, then re-run the command;
- build cross-repository graphs. One projection describes one `model.json`.

As with every other Model analysis, graph results are only as good as the
relations in the model: a tree-sitter-only model resolves calls by name, so an
ambiguous call may be missing; an SCIP index makes the edges exact.

## Why not `graph.json` compatibility

`model.graph/1` is Model's own projection, not Graphify's `graph.json`. The
design borrows a few useful ideas (a node/edge projection, community and
"god node" views) but there is **no dependency on Graphify and no
compatibility claim**: no shared schema, no reader or writer for its files,
and no promise that a future version will match it. If you need that format,
convert the projection yourself; this repository only guarantees the
deterministic `model.graph/1` contract described above.

## See also

- [cli.md](cli.md) — the `model graph` commands and their options
- [usage.md](usage.md) — analyses, sessions and on-demand import
- [mcp.md](mcp.md) — the in-memory MCP tools for paths, explanations,
  communities and god nodes
- [design.md](design.md) — where the projection layer sits
- [examples.md](examples.md) — a worked scenario on the bundled shop sample
