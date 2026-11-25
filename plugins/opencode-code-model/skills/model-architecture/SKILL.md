---
name: Model architecture
description: Use the in-memory code model for architecture questions: metrics, cycles, unused entities, degree ranking. Prefer model_analysis over ad-hoc scripts.
---

# model-architecture

Tools come from the `model` MCP server: `tools.model.status` and
`tools.model.analysis` in Code Mode (`model_status`, `model_analysis` as direct
tools).

For structural questions:

1. Call `status` to confirm data is loaded.
2. Use `analysis` with `name` set to `metrics`, `code-metrics`, `cycles`,
   `unused`, or `degrees` (optional `kinds`, `limit`).
3. Interpret results: `code-metrics` for LOC and cyclomatic complexity
   (ranked entities in `top`), cycles as components, `unused` as candidate dead
   code (verify before removing), and `degrees` as hotspots.

Combine with `query`/`context` to drill into specific nodes.
