---
name: Model context
description: Use the in-memory code model (model MCP tools) to answer questions about a symbol: definition, callers, callees, and nearby call sites. Prefer this over blind grepping when the model tools are available.
---

# model-context

The `model` MCP server exposes these tools. In Code Mode call them as
`tools.model.status`, `tools.model.query`, `tools.model.context`; as direct
tools they are named `model_status`, `model_query`, `model_context`.

When you need to understand a specific symbol (method, class, function):

1. Call `status` if unsure the model is loaded. If not loaded, rebuild it with
   `rebuild` (requires the server to have been started with `--root`) or build a
   `model.json` with the `model` CLI.
2. Call `context` with the exact `key` when known; otherwise use `query` with
   `kind`/`name` filters to find the key first.
3. Summarize: definition (`file:line`), incoming callers, outgoing callees, and
   the relevant `file:line` sites.

Prefer `context` over multiple raw searches: it returns the entity and its
neighbours in one call.
