---
name: model-architecture
description: Use the in-memory code model for architecture questions: metrics, cycles, unused entities, degree ranking. Prefer model_analysis over ad-hoc scripts.
---

# model-architecture

For structural questions:

1. Call `model_status` to confirm data is loaded.
2. Use `model_analysis` with `name` set to `metrics`, `code-metrics`, `cycles`, `unused`, or `degrees` (optional `kinds`, `limit`).
3. Interpret results: `code-metrics` for LOC and cyclomatic complexity (ranked entities in `top`), cycles as components, unused as candidate dead code (verify before removing), degrees as hotspots.

Combine with `model_query`/`model_context` to drill into specific nodes.