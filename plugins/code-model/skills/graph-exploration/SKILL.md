---
name: graph-exploration
description: Use the in-memory code model's graph tools (model_path, model_explain, model_communities, model_god_nodes, model_export_graph) to navigate relationships, find hotspots and export a data-only snapshot. Use for cross-cutting or architecture questions.
---

# graph-exploration

Use this when the question spans many entities: "how does A reach B?", "what is
this subsystem made of?", "where are the hotspots?". For a single symbol's
definition and immediate callers/callees, prefer `model-context`; for change
risk, prefer `model-impact`.

1. Call `model_status` if unsure a model is loaded. Resolve names to exact
   entity keys with `model_query` when needed.
2. Pick the graph question:
   - `model_path { from, to }` — shortest dependency chain between two keys.
     Optional `direction` (`callers`/`callees`/`both`, default `both`), `kinds`
     (empty = all relations), `depth`, `provenance`. Report each hop with its
     kind and `file:line`.
   - `model_explain { key }` — incoming/outgoing neighbours grouped by
     relation, with occurrence sites. Use `kinds` and `limit` to keep it short.
   - `model_god_nodes` — highest total-degree entities (in/out counts and
     `file:line`), optionally filtered by `kinds`; use `limit` (default 50).
   - `model_communities` — detected groups of related entities with sizes and
     cohesion; use `top` (default 50). Start here to orient yourself in an
     unfamiliar codebase.
3. For a reusable snapshot, call
   `model_export_graph { directory, top? }`. It creates the directory and
   writes `projection.json` (the `model.graph/1` data contract) and `REPORT.md`
   (summary, communities, ranked nodes and cross-community relations).
   It generates **no visualization**: no diagram, image or layout. Rendering,
   if desired, is the caller's job.

Always ground the answer in concrete keys and `file:line` from the results,
and say which filters (direction, kinds, depth, provenance) you used.
