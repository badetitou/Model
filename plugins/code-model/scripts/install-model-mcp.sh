#!/usr/bin/env bash
set -euo pipefail

# Install or locate the `model-mcp` binary for this plugin.
# Precedence:
#   1. $MODEL_MCP_BIN if it points to an executable file
#   2. `model-mcp` already on PATH
#   3. <plugin data>/bin/model-mcp if present
#   4. a local build under common targets (dev convenience)

DEST_DIR="${CLAUDE_PLUGIN_DATA:-${HOME}/.claude/plugin-data/code-model}/bin"
mkdir -p "${DEST_DIR}"
DEST="${DEST_DIR}/model-mcp"

if [[ -n "${MODEL_MCP_BIN:-}" && -x "${MODEL_MCP_BIN}" ]]; then
  if [[ "${MODEL_MCP_BIN}" != "${DEST}" ]]; then
    cp -f "${MODEL_MCP_BIN}" "${DEST}"
    chmod +x "${DEST}" || true
  fi
  echo "model-mcp: using MODEL_MCP_BIN=${MODEL_MCP_BIN}" >&2
  exit 0
fi

if command -v model-mcp >/dev/null 2>&1; then
  echo "model-mcp: found on PATH: $(command -v model-mcp)" >&2
  # Make a stable shim in plugin data so .mcp.json can point there.
  if [[ "$(command -v model-mcp)" != "${DEST}" ]]; then
    ln -sf "$(command -v model-mcp)" "${DEST}" 2>/dev/null || {
      cp -f "$(command -v model-mcp)" "${DEST}"
      chmod +x "${DEST}" || true
    }
  fi
  exit 0
fi

if [[ -x "${DEST}" ]]; then
  echo "model-mcp: using ${DEST}" >&2
  exit 0
fi

# Dev convenience: look for a built binary next to the plugin repo checkout.
# CLAUDE_PLUGIN_ROOT points at the plugin directory (…/plugins/code-model).
PLUGIN_ROOT="${CLAUDE_PLUGIN_ROOT:-}"
SEARCH_ROOTS=()
if [[ -n "${PLUGIN_ROOT}" ]]; then
  # repo root is typically parent of plugins/
  SEARCH_ROOTS+=("$(cd "${PLUGIN_ROOT}/../.." && pwd)")
fi
# Also try cwd if it looks like the Model repo.
if [[ -f "Cargo.toml" && -d "crates/model-mcp" ]]; then
  SEARCH_ROOTS+=("$(pwd)")
fi

for root in "${SEARCH_ROOTS[@]}"; do
  for candidate in \
    "${root}/target/release/model-mcp" \
    "${root}/target/debug/model-mcp"; do
    if [[ -x "${candidate}" ]]; then
      cp -f "${candidate}" "${DEST}"
      chmod +x "${DEST}" || true
      echo "model-mcp: installed from ${candidate} -> ${DEST}" >&2
      exit 0
    fi
  done
done

echo "model-mcp: not found. Build it with: cargo build -p model-mcp --release" >&2
echo "model-mcp: then set MODEL_MCP_BIN to the binary path, or place it at ${DEST}" >&2
# Do not fail hard: the MCP server may be installed system-wide later.
exit 0