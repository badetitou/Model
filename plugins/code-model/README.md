# code-model plugin

Claude Code plugin for code-model MCP tools. For OpenCode, see the equivalent
[`opencode-code-model`](../opencode-code-model/) plugin.

## Install

```text
/plugin install code-model --marketplace badetitou/model
```

This repository is the marketplace (`.claude-plugin/marketplace.json`, named `badetitou-model`), so the installed plugin id is `code-model@badetitou-model`.

## Setup (SessionStart)

On `SessionStart`, `hooks/hooks.json` runs `scripts/install-model-mcp.sh`:

- If `MODEL_MCP_BIN` is set and executable, it is copied to `${CLAUDE_PLUGIN_DATA}/bin/model-mcp`.
- Else if `model-mcp` is on `PATH`, a shim/symlink is placed in `${CLAUDE_PLUGIN_DATA}/bin/model-mcp`.
- Else it tries to copy from `../../target/{release,debug}/model-mcp` relative to the plugin root (repo dev convenience).

`.mcp.json` runs `${CLAUDE_PLUGIN_DATA}/bin/model-mcp --model ${CLAUDE_PLUGIN_DATA}/model.json`.

## Manual install (local test)

```bash
cargo build -p model-mcp --release
export MODEL_MCP_BIN="$PWD/target/release/model-mcp"
claude --plugin-dir ./plugins/code-model
```

Or copy the binary to `${CLAUDE_PLUGIN_DATA}/bin/model-mcp` yourself.

## Skills and agent

The plugin ships `skills/model-context`, `skills/model-impact`,
`skills/model-architecture` and `skills/graph-exploration` (paths,
neighbourhoods, communities, god nodes and data-only graph export), plus the
`impact-reviewer` agent in `agents/`. See the repo's `docs/mcp.md` for the full
tool list.

## Optional rebuild

`hooks/hooks.json` also has an async `Stop` hook (`scripts/rebuild.sh`) as a placeholder; for a live model prefer calling the `model_rebuild` MCP tool when the server was started with `--root`.