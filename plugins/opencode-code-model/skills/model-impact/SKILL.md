---
name: Model impact
description: Use the in-memory code model to assess change impact: who calls or can reach a seed, and what is affected. Use before refactors or risky edits.
---

# model-impact

Tools come from the `model` MCP server: `tools.model.status`,
`tools.model.query`, `tools.model.impact`, `tools.model.rebuild` in Code Mode
(`model_status`, `model_query`, `model_impact`, `model_rebuild` as direct tools).

Before changing a symbol or area:

1. Ensure the model is loaded (`status`). Rebuild if sources changed and the
   server was started with `--root`.
2. Call `impact` with `seeds` (entity keys). Choose a `direction`
   (`callers`, `callees`, or `both`) and optional `depth`/`kinds`.
3. Report seeds, counts, and the most relevant reached entities with
   `file:line`.

If you only know names, use `query` to resolve exact keys first. For a concise
review workflow, run the `/model-impact-review` command.
