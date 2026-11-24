#!/usr/bin/env bash
set -euo pipefail

# Optional: set MODEL_MCP_MODEL and MODEL_MCP_ROOT in the environment.
# This script is a placeholder for async rebuild; with stdio MCP, prefer calling
# model_rebuild from the agent, or run `model build` then restart the server.
if command -v model >/dev/null 2>&1 && [[ -n "${MODEL_MCP_ROOT:-}" ]]; then
  out="${MODEL_MCP_MODEL:-${MODEL_MCP_ROOT}/model.json}"
  model build "${MODEL_MCP_ROOT}" --out "${out}" --reindex never --quiet >/dev/null 2>&1 || true
fi
