# CLI (`model`)

The `model` command line builds a code model from sources and runs read-only
queries against a saved `model.json`. It is the stateless path: **each
invocation loads the JSON file from disk again**. For interactive sessions or
agents that make many calls, prefer the [MCP server](mcp.md) or the GraphQL
server, which keep the model in memory.

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

All data commands print JSON to stdout.

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

`model analysis <model.json> <metrics|cycles|unused|degrees>` with optional
`--kinds`, `--limit`.

## Timing

Set `MODEL_TIMING=1` to print how long `model.json` loading took (stderr). This
measures the cost of a stateless reload; on large exports it is often seconds
and gigabytes of RSS, which is why long-lived tools should keep the model in
memory.

## Exit codes and output

- Data: JSON on **stdout**.
- Progress and diagnostics: **stderr**.
- Non-zero exit on errors (missing file, unknown key with `--key`, parse errors, etc.).