---
name: model-impact
description: Use the in-memory code model to assess change impact: who calls/can reach a seed and what is affected. Use before refactors or risky edits.
---

# model-impact

Before changing a symbol or area:

1. Ensure the model is loaded (`model_status`). Rebuild if sources changed and the server was started with `--root`.
2. Call `model_impact` with `seeds` (entity keys). Choose `direction` (`callers`, `callees`, or `both`) and optional `depth`/`kinds`.
3. Report: seeds, counts, and the most relevant reached entities with `file:line`.

If you only know names, use `model_query` to resolve keys first.