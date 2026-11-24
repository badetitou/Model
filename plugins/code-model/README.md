# code-model plugin

Claude Code plugin for code-model MCP tools.

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

## Optional rebuild

`hooks/hooks.json` also has an async `Stop` hook (`scripts/rebuild.sh`) as a placeholder; for a live model prefer calling the `model_rebuild` MCP tool when the server was started with `--root`.