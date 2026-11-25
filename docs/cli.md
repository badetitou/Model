# CLI (`model`)

The `model` command line builds a code model from sources and runs read-only
queries against a saved `model.json`. It is the stateless path: **each
invocation loads the JSON file from disk again**. For interactive sessions or
agents that make many calls, prefer the [MCP server](mcp.md) or the GraphQL
server, which keep the model in memory. The `graph` commands are deterministic:
the same model and options produce byte-identical output, with no timestamps.
All `graph` commands emit JSON except `graph report`, which emits Markdown.

## Install

### From crates.io (when published)

The `model-cli` crate is listed in the repository release pipeline and published
to [crates.io](https://crates.io) as **`model-cli`** when a version tag (e.g.
`v0.8.0`) is released. **Whether a given version is already on crates.io depends
on what has been published:** if you see `model-cli` on crates.io for the version
you need, you can install the `model` binary:

```sh
cargo install model-cli
```

Then run `model --help`. (The binary/package is named `model-cli`; the installed
command is `model`.)

If installation fails because the crate/version is not published yet, use a
source checkout instead.

### From a source checkout

From the repository root (any branch with the crate):

```sh
cargo run -q -p model-cli -- --help
```

Or build and install from the local tree:

```sh
cargo install --path crates/model-cli
```

## Build

Example using a source checkout:

Build a model from a source tree (progress on stderr, JSON summary on stdout
with `--json`):

```sh
cargo run -q -p model-cli -- detect ../my-project --json
cargo run -q -p model-cli -- build ../my-project --out model.json --reindex never
```

By default `build` uses `--reindex never` (no indexer run). Use
`--reindex if-missing|if-stale|always` only when you intentionally want indexers
to run. Use `--out -` to write JSON to stdout (not compatible with `--json`).

Common flags on `build`:

| Flag | Effect |
|---|---|
| `--out <path\|->` | Write the model (`model.json`, atomic write). |
| `--reindex …` | Indexer policy (`never`, `if-missing`, `if-stale`, `always`). |
| `--no-scip` | Tree-sitter only (no SCIP linking). |
| `--no-cache` / `--from-cache` | Document cache control. |
| `--scope <path>` | Limit extraction to paths (repeatable). |
| `--languages <lang>` | Limit to detected language ids (repeatable). |
| `--index-command LANGUAGE=COMMAND` | Custom index command (also `MODEL_INDEX_COMMAND`). |
| `--fragment-cache <dir>` | Per-file fragment cache directory. |
| `--quiet` | Less progress on stderr. |
| `--json` / `--pretty` | Machine-readable build summary. |

## Query a saved model

All data commands print JSON to stdout (except `graph report`, below, which
prints Markdown).

```sh
cargo run -q -p model-cli -- query model.json --kind method --limit 20
cargo run -q -p model-cli -- query model.json --key "<entity key>"
cargo run -q -p model-cli -- context model.json --key "<entity key>"
cargo run -q -p model-cli -- impact model.json --seed "<entity key>" --direction callers
cargo run -q -p model-cli -- analysis model.json cycles
```

### `query`

Filters (combine as needed):

- `--key <key>`: exact key; if the key is missing this is an error unless you also filter and get no rows.
- `--kind <kind>`: archetype or fine kind (e.g. `method`, `http.endpoints`).
- `--name <substring>`: case-insensitive substring of the name.
- `--file <substring>`: substring of the source path.
- `--relation <kind>`: repeatable; include relation information in results when used.
- `--direction in|out|both`: limit relations to one side.
- `--limit <n>`: default is 50 (0 means no limit in the API; the CLI default applies unless overridden).

### `context`

One call returns the entity plus incoming and outgoing neighbours (default
relation kinds: calls and references). Use `--relation` to restrict, `--limit`
for neighbours per side (default 20).

### `impact`

Propagation from one or more `--seed <key>` (repeatable). Options:
`--direction callers|callees|both`, `--depth <n>` (omit for full reach),
`--kinds <comma,list>`, `--limit <n>` (default 50).

### `analysis`

`model analysis <model.json> <metrics|code-metrics|cycles|unused|degrees>` with
optional `--kinds`, `--limit`. `code-metrics` reports LOC and cyclomatic
complexity (model-wide, by file/language, and the `--limit` most complex
entities).

## Graph analyses

`model graph <projection|report|path|explain|communities> <model.json> [options]`
projects a saved model into the deterministic `model.graph/1` graph and runs
read-only graph analyses over it. The format, invariants and limits are in
[graph.md](graph.md). `projection`, `path`, `explain` and `communities` print
JSON to stdout; **`report` prints Markdown**. `projection` and `report` accept
`--out <file|->` to write to a file (`-` keeps stdout); the other three do not
have an `--out` flag.

```sh
cargo run -q -p model-cli -- graph projection model.json --out graph.json --pretty
cargo run -q -p model-cli -- graph report model.json --top 20 --out report.md
cargo run -q -p model-cli -- graph path model.json --from '<key A>' --to '<key B>' --direction both --depth 5
cargo run -q -p model-cli -- graph explain model.json --key '<key>' --limit 20
cargo run -q -p model-cli -- graph communities model.json --top 10 --pretty
```

### `graph projection`

Write the full `model.graph/1` document (`schema`, `summary`, `communities`,
`nodes`, `edges`). Node ids are stable entity keys; every edge endpoint is a
node, externals included; lines are 1-based. Omitting a filter keeps all
relation kinds and all provenances.

- `--out <path|->`: destination (default stdout; atomic file write).
- `--kinds <kind,kind>`: comma-separated relation kinds to project (e.g. `calls,references`). An empty list selects no kinds.
- `--provenance <name>`: keep only relations with that provenance; `tree_sitter`, `scip`, `frontend` or `derived`.
- `--no-communities`: emit no communities, `modularity: 0.0`, and node `community: 0` with an empty label.
- `--no-props`: emit `{}` for every node's `props`.
- `--no-sites`: emit empty `sites` and a `summary.sites` count of `0` (weights stay).
- `--pretty`: indent the JSON. Deterministic either way.

### `graph report`

**Markdown**, not JSON: a summary, the communities, the most connected nodes
(god nodes, from `Degrees::ranked()`), the strongest cross-community relations
and suggested `context`/`impact` questions. It builds its projection with the
default options (all kinds and provenances, communities/properties/sites
included), so it has no filter flags.

- `--out <path|->`: destination (default stdout; atomic file write).
- `--top <n>`: cap the ranked and cross-community lists (default 10; `0` means no cap).

### `graph path`

A breadth-first shortest chain of relations between two entities, with stable
tie-breaking. An unreachable pair returns empty `nodes` and `hops`.

- `--from <key>` and `--to <key>`: stable entity keys (required).
- `--kinds <kind,kind>`: relation kinds to traverse; defaults to `calls,references`. An empty list selects no kinds.
- `--depth <n>`: maximum number of hops; `0` (or omitting the flag) means no cap.
- `--direction callers|callees|both`: which side of the graph to walk (default `both`).
- `--provenance <name>`: `tree_sitter`, `scip`, `frontend` or `derived`; all provenances by default.
- `--pretty`: indent the JSON.

### `graph explain`

The neighborhood of one entity: aggregated incoming and outgoing neighbours
and their occurrence sites. Degrees count all matching relations even when the
neighbour list is capped.

- `--key <key>`: stable entity key (required).
- `--kinds <kind,kind>`: relation kinds to include; all by default. An empty list selects no kinds.
- `--limit <n>`: maximum neighbours to report (`0` or omitted means no cap).
- `--pretty`: indent the JSON.

There is no provenance filter on this command.

### `graph communities`

The deterministic Louvain-like community partition alone: numeric ids, labels,
sizes, members, internal weight and degree, plus `modularity` and the full
`total` count. The CLI exposes no kind or provenance filter here.

- `--top <n>`: truncate the emitted `communities` array; `total` keeps the full count. `0` (the default) means no cap.
- `--pretty`: indent the JSON.

## Timing

Set `MODEL_TIMING=1` to print how long `model.json` loading took (stderr). This
measures the cost of a stateless reload; on large exports it is often seconds
and gigabytes of RSS, which is why long-lived tools should keep the model in
memory.

## Exit codes and output

- Data: JSON on **stdout**, except `graph report`, which prints Markdown.
- Progress and diagnostics: **stderr**.
- Non-zero exit on errors (missing file, unknown key with `--key`, parse errors, etc.).