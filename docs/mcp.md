# MCP server and Claude Code plugin

`model-mcp` is a stdio MCP server that keeps a `Model` in memory so tools do not
pay the cost of reloading `model.json` on every call. It is meant to sit behind
an MCP client (Claude Code, other editors/tools that support stdio MCP).

## Binary

### Not published to crates.io (today)

`model-mcp` is **not** listed in the repository’s release script
(`scripts/crates.sh`), so the automated tag release does **not** publish it to
crates.io. Installing via `cargo install model-mcp` is not a supported path
unless you build from source (or a maintainer publishes it separately).

### From a source checkout

From the repository root:

```sh
cargo build -p model-mcp --release
```

The binary is at `target/release/model-mcp` (or `target/debug/model-mcp` if you
omit `--release`).

Run manually (example):

```sh
./target/release/model-mcp --model /path/to/model.json
```

With a source root configured, `model_rebuild` can rebuild from sources
(and writes the JSON to `--model`):

```sh
./target/release/model-mcp --model /path/to/model.json --root /path/to/repo --reindex never
```

Useful flags mirror parts of `model build`: `--root`, `--reindex`,
`--no-scip`, `--no-cache`, `--from-cache`, `--scope`, `--languages`,
`--index-command LANGUAGE=COMMAND`, `--fragment-cache <dir>`. Run
`model-mcp --help` for the full list.

## MCP tools (overview)

The server exposes tools (names as registered by the server):

| Tool | Purpose |
|---|---|
| `model_status` | Whether a model is loaded, origin, counts, rebuilds. |
| `model_query` | Find entities by key/filters (same semantics as `model query`). |
| `model_context` | One entity plus callers/callees and `file:line` sites. |
| `model_impact` | Propagation from seeds (direction, depth, kinds). |
| `model_analysis` | `metrics`, `code-metrics` (LOC and cyclomatic complexity), `cycles`, `unused`, or `degrees`. |
| `model_path` | Shortest dependency path between two exact entity keys (direction, kinds, depth, provenance). |
| `model_explain` | One entity's incoming/outgoing relations grouped by kind, with occurrence sites (`file:line`). |
| `model_communities` | Detected communities (groups of related entities) with sizes and cohesion. |
| `model_god_nodes` | Most connected entities by total degree, with in/out counts and `file:line`. |
| `model_export_graph` | Write `projection.json` and `REPORT.md` under a directory (data only, no visualization). |
| `model_rebuild` | Reload JSON from disk or rebuild from `--root` if configured, then swap the model in memory. |

Tool results are JSON text content (pretty-printed). On errors the server returns
an MCP error with a message.

## Graph exploration

The five graph tools answer questions that span many entities rather than one:

1. Use `model_query` to resolve names to exact keys when needed.
2. `model_path` finds the shortest dependency chain between two keys;
   `direction` accepts `callers`, `callees`, or `both` (default), `kinds`
   defaults to all relations, and `depth`/`provenance` narrow the walk.
3. `model_explain` returns the local neighbourhood of one key with relation
   sites; `model_god_nodes` ranks structural hotspots by in/out degree.
4. `model_communities` lists the model's partition into related groups, useful
   for orientation before drilling into a subsystem.

`model_export_graph { "directory": "..." }` creates the directory if needed and
writes two files:

- `projection.json` — a deterministic `model.graph/1` document (nodes, edges,
  communities, summary) that any client can read.
- `REPORT.md` — a Markdown report with the summary, communities, most connected
  nodes and strongest cross-community relations.

No visualization is generated here: the server writes data and Markdown only.
Rendering (graphs, diagrams, images) is left to the caller.

## Claude Code plugin (`code-model`)

A plugin skeleton lives in [`plugins/code-model/`](../plugins/code-model/)
(relative to the repo root). It provides:

- `.claude-plugin/plugin.json` (metadata)
- `.mcp.json` that launches the server as:
  `${CLAUDE_PLUGIN_DATA}/bin/model-mcp --model ${CLAUDE_PLUGIN_DATA}/model.json`
- `hooks/hooks.json`:
  - **SessionStart** (sync): runs `scripts/install-model-mcp.sh` to place `model-mcp`
    under `${CLAUDE_PLUGIN_DATA}/bin/` (see below).
  - **Stop** (async): runs `scripts/rebuild.sh` as a placeholder (optional).

### Installing `model-mcp` for the plugin

`install-model-mcp.sh` chooses the binary in this order:

1. `MODEL_MCP_BIN` if set and executable (copied into plugin data).
2. `model-mcp` found on `PATH` (shim/symlinked into plugin data when useful).
3. A built binary from a repo checkout next to the plugin: looks under
   `../../target/release/model-mcp` and `../../target/debug/model-mcp` relative
   to `CLAUDE_PLUGIN_ROOT` (dev convenience).
4. If nothing is found, it prints instructions and exits successfully so the
   session can continue; the MCP server will fail to start until the binary
   exists at `${CLAUDE_PLUGIN_DATA}/bin/model-mcp`.

### Local testing

Build the binary and point the installer at it:

```sh
cargo build -p model-mcp --release
export MODEL_MCP_BIN="$PWD/target/release/model-mcp"
claude --plugin-dir ./plugins/code-model
```

Or copy the binary manually:

```sh
mkdir -p "${CLAUDE_PLUGIN_DATA:-$HOME/.claude/plugin-data/code-model}/bin"
cp target/release/model-mcp "${CLAUDE_PLUGIN_DATA:-$HOME/.claude/plugin-data/code-model}/bin/model-mcp"
chmod +x "${CLAUDE_PLUGIN_DATA:-$HOME/.claude/plugin-data/code-model}/bin/model-mcp"
```

You still need a `model.json` at `${CLAUDE_PLUGIN_DATA}/model.json` for the
default `.mcp.json`, or adjust the args to pass `--root ...` and let
`model_rebuild` build it (requires extending the server invocation/configuration
to include `--root`; see `model-mcp --help`).

### Skills and agent

The plugin ships skills under `skills/` (`model-context`, `model-impact`,
`model-architecture`, `graph-exploration`) and an agent
`agents/impact-reviewer.md` to guide usage of the MCP tools. Hooks run with the
plugin enabled; keep plugin state under `${CLAUDE_PLUGIN_DATA}`, not inside the
plugin directory.

### Validation

From the repo root (if `claude` CLI is installed):

```sh
claude plugin validate --strict plugins/code-model
```

## OpenCode plugin (`opencode-code-model`)

The OpenCode equivalent lives in [`plugins/opencode-code-model/`](../plugins/opencode-code-model/)
and is loaded by the repository's `opencode.json`. It registers the same `model`
MCP server plus the four skills (`model-context`, `model-impact`,
`model-architecture`, `graph-exploration`) and a `/model-impact-review` command
implementing the `impact-reviewer` workflow. The agent file is also available
at `.opencode/agents/impact-reviewer.md` for direct subagent use.

Binary discovery happens in plugin `setup` (`options.binary`, `$MODEL_MCP_BIN`,
`PATH`, then `target/{release,debug}/model-mcp`); `model.json` defaults to
`<project>/model.json` and `--root` defaults to the project, enabling
`model_rebuild`. An existing `mcp.servers.model` configuration is preserved.
OpenCode V2 has no plugin `SessionStart`/`Stop` hooks, so use `model_rebuild`
for a live refresh. See the plugin README for all options.

## Relationship to the CLI

| Concern | `model` CLI | `model-mcp` |
|---|---|---|
| State | Stateless, reloads JSON per command | Model kept in memory |
| Output | JSON on stdout (plus progress on stderr) | MCP tool results (JSON text) |
| Best for | Scripts, CI, one-off queries | Agents, editors, repeated queries |

Use the CLI to produce and inspect `model.json`; use the MCP server for
low-latency, multi-query sessions.