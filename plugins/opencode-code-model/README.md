# opencode-code-model

OpenCode plugin for [`model-mcp`](../../docs/mcp.md), the in-memory code model
server. It is the OpenCode counterpart of the Claude Code plugin in
[`../code-model`](../code-model/).

## Load

The repository root has an `opencode.json` that loads this local plugin:

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "plugins": [{ "package": "./plugins/opencode-code-model", "options": {} }],
}
```

For another project, point `plugins` at this directory (or publish the package
and use its npm name). Config changes reload the plugin automatically; no
restart is needed.

## What it registers

- An MCP server named `model` running `model-mcp --model <model.json> --root <project> --reindex never`.
  Its tools are `model_status`, `model_query`, `model_context`, `model_impact`,
  `model_analysis`, `model_path`, `model_explain`, `model_communities`,
  `model_god_nodes`, `model_export_graph` and `model_rebuild`. Under Code Mode,
  call them as `tools.model.<tool>`.
- Four skills: `model-context`, `model-impact`, `model-architecture` and
  `graph-exploration` (a port of the Claude plugin's skills).
- The `/model-impact-review` command, implementing the `impact-reviewer`
  workflow. OpenCode V2 has no plugin API to register agents, so copy
  [`agents/impact-reviewer.md`](agents/impact-reviewer.md) into
  `.opencode/agents/` to expose it as a subagent (already done at this
  repository's root).

Unlike Claude Code, OpenCode V2 has no `SessionStart`/`Stop` shell hooks for
plugins: binary discovery happens in plugin `setup`, and a live refresh is done
by calling `model_rebuild` after code changes (the server needs `--root`, which
this plugin passes by default).

## Binary and model resolution

The plugin looks for `model-mcp` in this order:

1. `options.binary`, then `$MODEL_MCP_BIN` (relative paths resolve from the project).
2. `model-mcp` on `PATH`.
3. `<project>/target/{release,debug}/model-mcp`, then the same paths next to the plugin.

If none is found, the server is registered as disabled and a warning is logged:

```sh
cargo build -p model-mcp --release
export MODEL_MCP_BIN="$PWD/target/release/model-mcp"
```

`model.json` defaults to `<project>/model.json` (`$MODEL_MCP_MODEL` or
`options.model` override it). With `--root` set, `model-mcp` builds the model
from sources at startup when the JSON is missing. Do not index fixtures; the
default `--reindex never` only reuses existing indexes.

## Options

| Option | Environment | Default | Meaning |
| --- | --- | --- | --- |
| `binary` | `MODEL_MCP_BIN` | PATH, then local builds | `model-mcp` executable |
| `model` | `MODEL_MCP_MODEL` | `<project>/model.json` | exported model |
| `root` | `MODEL_MCP_ROOT` | project root | source root for `model_rebuild`; `false` disables it |
| `reindex` | — | `never` | `never`, `if-missing`, `if-stale`, `always` |
| `codemode` | — | `true` | expose MCP tools through Code Mode |

```jsonc
{
  "plugins": [
    {
      "package": "./plugins/opencode-code-model",
      "options": { "model": "/abs/path/model.json", "root": false }
    }
  ],
}
```

An existing `mcp.servers.model` entry is never overwritten: configure the
server yourself if you need custom arguments.

## Validation

```sh
opencode mcp list
opencode api get /api/skill
```

Check that the `model` server is connected, then ask the agent to use the
`model-context` skill or run `/model-impact-review`.
