---
name: model-context
description: Use the in-memory code model (via model_mcp tools) to answer questions about a symbol: definition, callers, callees, and nearby call sites. Prefer this over blind grepping when model tools are available.
---

# model-context

When you need to understand a specific symbol (method, class, function):

1. Call `model_status` (if unsure the model is loaded). If not loaded, ask the user to run `model_rebuild` or start the MCP with `--model`/`--root`.
2. Call `model_context` with the exact `key` when known; otherwise use `model_query` with `kind`/`name` filters to find the key first.
3. Summarize: definition (file:line), incoming callers, outgoing callees, and relevant `file:line` sites.

Prefer `model_context` over multiple raw searches—it returns entity + neighbors in one call.